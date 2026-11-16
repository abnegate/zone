import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { overview } from '../../../test/analytics';
import type { Overview, TimePeriod } from '../types';

const mockOverview = mock();

mock.module('../../../api/analytics', () => ({
  analyticsApi: { overview: mockOverview },
  LOAD_FAILED: 'Failed to load analytics',
}));

let workspace: { id: string } | null = { id: 'workspace-1' };
let workspaceLoading = false;

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: workspace, loading: workspaceLoading }),
}));

let useAnalytics: typeof import('./useAnalytics').useAnalytics;

beforeAll(async () => {
  ({ useAnalytics } = await import('./useAnalytics'));
});

afterAll(() => {
  mock.restore();
});

const QUIET = { defaultOptions: { queries: { retry: false, gcTime: 0 } } };

function provider(client: QueryClient = new QueryClient(QUIET)) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

function pending<T>(): { promise: Promise<T>; resolve(value: T): void } {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

describe('useAnalytics', () => {
  beforeEach(() => {
    mockOverview.mockReset();
    workspace = { id: 'workspace-1' };
    workspaceLoading = false;
  });

  it('loads the overview for the workspace and period', async () => {
    const loaded = overview();
    mockOverview.mockResolvedValue(loaded);

    const { result } = renderHook(() => useAnalytics('month'), { wrapper: provider() });

    expect(result.current.loading).toBe(true);
    await waitFor(() => expect(result.current.overview).toEqual(loaded));
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
    expect(mockOverview).toHaveBeenCalledWith('workspace-1', 'month', expect.any(AbortSignal));
  });

  it('asks for nothing without a workspace and says so once workspaces have loaded', () => {
    workspace = null;

    const { result } = renderHook(() => useAnalytics('week'), { wrapper: provider() });

    expect(mockOverview).not.toHaveBeenCalled();
    expect(result.current.missingWorkspace).toBe(true);
    expect(result.current.loading).toBe(false);
  });

  it('waits on the workspace rather than calling it missing while it loads', () => {
    workspace = null;
    workspaceLoading = true;

    const { result } = renderHook(() => useAnalytics('week'), { wrapper: provider() });

    expect(result.current.missingWorkspace).toBe(false);
    expect(result.current.loading).toBe(true);
  });

  it('surfaces the server error', async () => {
    mockOverview.mockRejectedValue(new Error('Failed to load analytics: period must be one of'));

    const { result } = renderHook(() => useAnalytics('week'), { wrapper: provider() });

    await waitFor(() =>
      expect(result.current.error).toBe('Failed to load analytics: period must be one of')
    );
    expect(result.current.overview).toBeUndefined();
  });

  it('keeps the last period on screen while the next one loads', async () => {
    const week = overview();
    const month = pending<Overview>();
    mockOverview.mockResolvedValueOnce(week).mockReturnValueOnce(month.promise);

    const { result, rerender } = renderHook(
      ({ period }: { period: TimePeriod }) => useAnalytics(period),
      { wrapper: provider(), initialProps: { period: 'week' } }
    );
    await waitFor(() => expect(result.current.overview).toEqual(week));

    rerender({ period: 'month' });

    await waitFor(() => expect(result.current.refreshing).toBe(true));
    expect(result.current.overview).toEqual(week);
    expect(result.current.loading).toBe(false);

    const monthly = overview({ period: 'month' });
    month.resolve(monthly);
    await waitFor(() => expect(result.current.overview).toEqual(monthly));
    expect(result.current.refreshing).toBe(false);
  });

  it('never shows one workspace the numbers of another', async () => {
    const first = overview();
    const second = pending<Overview>();
    mockOverview.mockResolvedValueOnce(first).mockReturnValueOnce(second.promise);

    const { result, rerender } = renderHook(() => useAnalytics('week'), {
      wrapper: provider(),
    });
    await waitFor(() => expect(result.current.overview).toEqual(first));

    workspace = { id: 'workspace-2' };
    rerender();

    await waitFor(() => expect(mockOverview).toHaveBeenCalledTimes(2));
    expect(result.current.overview).toBeUndefined();
    expect(result.current.loading).toBe(true);
    second.resolve(overview());
  });

  it('retries a failed load', async () => {
    mockOverview.mockRejectedValueOnce(new Error('down')).mockResolvedValueOnce(overview());

    const { result } = renderHook(() => useAnalytics('week'), { wrapper: provider() });
    await waitFor(() => expect(result.current.error).toBe('down'));

    result.current.retry();

    await waitFor(() => expect(result.current.overview).toBeDefined());
    expect(result.current.error).toBeNull();
  });
});
