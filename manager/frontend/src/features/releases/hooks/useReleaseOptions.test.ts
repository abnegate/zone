import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { options } from '../../../test/releases';
import { retryRead } from '../../github';
import type { OptionsResponse } from '../types';

const mockOptions = mock();

mock.module('../../../api/releases', () => ({
  releasesApi: { options: mockOptions },
}));

let workspace: { id: string } | null = { id: 'workspace-1' };

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: workspace, loading: false }),
}));

let useReleaseOptions: typeof import('./useReleaseOptions').useReleaseOptions;
let releaseOptionsKey: typeof import('./keys').releaseOptionsKey;
let NO_WORKSPACE: string;

beforeAll(async () => {
  ({ useReleaseOptions } = await import('./useReleaseOptions'));
  ({ releaseOptionsKey } = await import('./keys'));
  ({ NO_WORKSPACE } = await import('../../github'));
});

afterAll(() => {
  mock.restore();
});

const QUIET = {
  defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
};

function provider(client: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

const reloaded: OptionsResponse = {
  ...options,
  sources: options.sources.map((source) => ({
    ...source,
    latest_tag: 'v1.2.4',
    next_tag: 'v1.2.5',
  })),
};

describe('useReleaseOptions', () => {
  let client: QueryClient;

  beforeEach(() => {
    mockOptions.mockReset();
    workspace = { id: 'workspace-1' };
    client = new QueryClient(QUIET);
  });

  it('reads nothing while the wizard is closed', async () => {
    const { result } = renderHook(() => useReleaseOptions(false), { wrapper: provider(client) });
    await new Promise((settle) => setTimeout(settle, 50));
    expect(mockOptions).not.toHaveBeenCalled();
    expect(result.current.sources).toEqual([]);
    expect(result.current.loading).toBe(false);
  });

  it('reads the options once the wizard opens', async () => {
    mockOptions.mockResolvedValue(options);
    const { result, rerender } = renderHook(({ open }) => useReleaseOptions(open), {
      wrapper: provider(client),
      initialProps: { open: false },
    });
    rerender({ open: true });
    expect(result.current.loading).toBe(true);
    await waitFor(() => expect(result.current.sources).toEqual(options.sources));
    expect(result.current.options).toEqual(options);
    expect(mockOptions.mock.calls[0].slice(0, 2)).toEqual(['workspace-1', {}]);
  });

  it('retries a failed read a few times like any other read', async () => {
    mockOptions.mockResolvedValue(options);
    const { result } = renderHook(() => useReleaseOptions(true), { wrapper: provider(client) });
    await waitFor(() => expect(result.current.options).toEqual(options));
    const query = client.getQueryCache().find({ queryKey: releaseOptionsKey('workspace-1') });
    expect(query?.observers[0].options.retry).toBe(retryRead);
  });

  it('reports a missing workspace at once instead of retrying it', async () => {
    const missing = new ApiError('Failed to fetch release options: 404', HttpStatus.NotFound);
    mockOptions.mockRejectedValue(missing);
    const { result } = renderHook(() => useReleaseOptions(true), { wrapper: provider(client) });
    await waitFor(() => expect(result.current.error).toBe(missing.message));
    expect(mockOptions).toHaveBeenCalledTimes(1);
  });

  it('reloads past the server cache and keeps the new options', async () => {
    mockOptions.mockResolvedValueOnce(options).mockResolvedValueOnce(reloaded);
    const { result } = renderHook(() => useReleaseOptions(true), { wrapper: provider(client) });
    await waitFor(() => expect(result.current.options).toEqual(options));

    await act(() => result.current.reload());

    expect(mockOptions.mock.calls[1]).toEqual(['workspace-1', { refresh: true }]);
    await waitFor(() => expect(result.current.options).toEqual(reloaded));
    expect(client.getQueryData(releaseOptionsKey('workspace-1'))).toEqual(reloaded);
  });

  it('refuses to reload without a workspace', async () => {
    workspace = null;
    const { result } = renderHook(() => useReleaseOptions(true), { wrapper: provider(client) });
    await expect(result.current.reload()).rejects.toThrow(NO_WORKSPACE);
    expect(mockOptions).not.toHaveBeenCalled();
  });
});
