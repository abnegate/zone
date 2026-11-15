import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  mock,
  vi,
} from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { listing, pipelineRun, release } from '../../../test/releases';
import { fakeTimers } from '../../../test/timers';
import { GATEWAY_BACKOFF_MS } from '../../github';
import type { Release, ReleasesResponse } from '../types';

const mockList = mock();

mock.module('../../../api/releases', () => ({
  releasesApi: { list: mockList },
}));

let workspace: { id: string } | null = { id: 'workspace-1' };

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: workspace, loading: false }),
}));

let useReleases: typeof import('./useReleases').useReleases;
let RELEASES_POLL_MS: number;
let releaseWindowKey: typeof import('./keys').releaseWindowKey;

beforeAll(async () => {
  ({ useReleases, RELEASES_POLL_MS } = await import('./useReleases'));
  ({ releaseWindowKey } = await import('./keys'));
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

async function advance(milliseconds: number): Promise<void> {
  await act(async () => {
    vi.advanceTimersByTime(milliseconds);
  });
  await act(async () => {
    vi.advanceTimersByTime(0);
  });
}

function hide(hidden: boolean): void {
  Object.defineProperty(document, 'visibilityState', {
    configurable: true,
    get: () => (hidden ? 'hidden' : 'visible'),
  });
  window.dispatchEvent(new Event('visibilitychange'));
}

const gateway = new ApiError('Failed to fetch releases: 502', HttpStatus.BadGateway);
const current = listing([release()]);
const refreshed = listing([release({ id: 43, tag: 'v1.2.4' }), release()]);

function checked(subject: Release, at: string, state: Release['state']): Release {
  const run = pipelineRun({ state });
  return {
    ...subject,
    state,
    succeeded: state === 'succeeded',
    complete: state === 'succeeded',
    pipeline: { ...subject.pipeline, runs: [run], checked_at: at },
  };
}

describe('useReleases', () => {
  let client: QueryClient;

  beforeEach(() => {
    mockList.mockReset();
    workspace = { id: 'workspace-1' };
    client = new QueryClient(QUIET);
  });

  afterEach(async () => {
    await client.cancelQueries();
    client.clear();
    vi.useRealTimers();
    hide(false);
  });

  it('reads nothing without a workspace', async () => {
    workspace = null;
    const { result } = renderHook(() => useReleases(), { wrapper: provider(client) });
    await new Promise((settle) => setTimeout(settle, 50));
    expect(mockList).not.toHaveBeenCalled();
    expect(result.current.releases).toEqual([]);
    expect(result.current.loading).toBe(false);
  });

  it('lists the releases of the workspace', async () => {
    mockList.mockResolvedValue(current);
    const { result } = renderHook(() => useReleases(), { wrapper: provider(client) });
    expect(result.current.loading).toBe(true);
    await waitFor(() => expect(result.current.releases).toEqual(current.releases));
    expect(result.current.listing).toEqual(current);
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
    expect(mockList).toHaveBeenCalledWith('workspace-1', { days: undefined }, expect.anything());
  });

  it('asks for the window it is given', async () => {
    mockList.mockResolvedValue(current);
    renderHook(() => useReleases(14), { wrapper: provider(client) });
    await waitFor(() => expect(mockList).toHaveBeenCalled());
    expect(mockList.mock.calls[0].slice(0, 2)).toEqual(['workspace-1', { days: 14 }]);
    await waitFor(() =>
      expect(client.getQueryData(releaseWindowKey('workspace-1', 14))).toEqual(current)
    );
  });

  it('reports a failed read', async () => {
    mockList.mockRejectedValue(new ApiError('Workspace not found', HttpStatus.NotFound));
    const { result } = renderHook(() => useReleases(), { wrapper: provider(client) });
    await waitFor(() => expect(result.current.error).toBe('Workspace not found'));
    expect(result.current.unavailable).toBe(false);
    expect(mockList).toHaveBeenCalledTimes(1);
  });

  it('keeps the releases it read while GitHub is unreachable, until a read succeeds', async () => {
    mockList
      .mockResolvedValueOnce(current)
      .mockRejectedValueOnce(gateway)
      .mockResolvedValue(refreshed);
    const { result, unmount } = renderHook(() => useReleases(), { wrapper: provider(client) });
    await waitFor(() => expect(result.current.releases).toEqual(current.releases));

    act(() => {
      void client.refetchQueries();
    });

    await waitFor(() => expect(result.current.unavailable).toBe(true));
    expect(result.current.error).toBeNull();
    expect(result.current.releases).toEqual(current.releases);

    act(() => {
      void client.refetchQueries();
    });

    await waitFor(() => expect(result.current.releases).toEqual(refreshed.releases));
    expect(result.current.unavailable).toBe(false);
    unmount();
  });

  it('keeps a pipeline checked later than the list it polls', async () => {
    const subject = release({ published_at: new Date().toISOString() });
    const settled = checked(subject, '2026-11-13T09:10:00Z', 'succeeded');
    const behind = checked(subject, '2026-11-13T09:05:00Z', 'running');
    client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryData(releaseWindowKey('workspace-1'), listing([settled]));
    mockList.mockResolvedValue(listing([behind]));
    renderHook(() => useReleases(), { wrapper: provider(client) });

    await waitFor(() => expect(mockList).toHaveBeenCalledTimes(1));
    await new Promise((settle) => setTimeout(settle, 50));

    const kept = client.getQueryData<ReleasesResponse>(releaseWindowKey('workspace-1'));
    expect(kept?.releases[0].pipeline).toEqual(settled.pipeline);
    expect(kept?.releases[0].state).toBe('succeeded');
  });

  it('takes a pipeline the list checked later than the cache', async () => {
    const subject = release({ published_at: new Date().toISOString() });
    const running = checked(subject, '2026-11-13T09:05:00Z', 'running');
    const settled = checked(subject, '2026-11-13T09:10:00Z', 'succeeded');
    client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryData(releaseWindowKey('workspace-1'), listing([running]));
    mockList.mockResolvedValue(listing([settled]));
    const { result } = renderHook(() => useReleases(), { wrapper: provider(client) });

    await waitFor(() => expect(result.current.releases[0]?.state).toBe('succeeded'));
    expect(result.current.releases[0].pipeline).toEqual(settled.pipeline);
  });

  it('refreshes past the server cache and keeps what it read', async () => {
    mockList.mockResolvedValueOnce(current).mockResolvedValueOnce(refreshed);
    const { result } = renderHook(() => useReleases(), { wrapper: provider(client) });
    await waitFor(() => expect(result.current.listing).toEqual(current));

    act(() => result.current.refresh());

    await waitFor(() => expect(result.current.listing).toEqual(refreshed));
    expect(mockList).toHaveBeenCalledTimes(2);
    expect(mockList.mock.calls[1].slice(0, 2)).toEqual([
      'workspace-1',
      { days: undefined, refresh: true },
    ]);
    expect(client.getQueryData<ReleasesResponse>(releaseWindowKey('workspace-1'))).toEqual(
      refreshed
    );
    expect(result.current.refreshing).toBe(false);
  });

  it('reports a failed refresh until a later read succeeds', async () => {
    mockList
      .mockResolvedValueOnce(current)
      .mockRejectedValueOnce(new ApiError('Rate limited', HttpStatus.BadGateway))
      .mockResolvedValue(refreshed);
    const { result } = renderHook(() => useReleases(), { wrapper: provider(client) });
    await waitFor(() => expect(result.current.listing).toEqual(current));

    act(() => result.current.refresh());

    await waitFor(() => expect(result.current.error).toBe('Rate limited'));
    expect(result.current.releases).toEqual(current.releases);

    act(() => {
      void client.refetchQueries();
    });

    await waitFor(() => expect(result.current.releases).toEqual(refreshed.releases));
    expect(result.current.error).toBeNull();
  });

  it('says GitHub is unreachable when a refresh answers 502', async () => {
    mockList.mockResolvedValueOnce(current).mockRejectedValueOnce(gateway);
    const { result } = renderHook(() => useReleases(), { wrapper: provider(client) });
    await waitFor(() => expect(result.current.listing).toEqual(current));

    act(() => result.current.refresh());

    await waitFor(() => expect(result.current.unavailable).toBe(true));
    expect(result.current.releases).toEqual(current.releases);
  });

  it('does nothing when asked to refresh without a workspace', async () => {
    workspace = null;
    const { result } = renderHook(() => useReleases(), { wrapper: provider(client) });
    act(() => result.current.refresh());
    await new Promise((settle) => setTimeout(settle, 50));
    expect(mockList).not.toHaveBeenCalled();
  });

  it('reads the list again every two minutes', async () => {
    await fakeTimers();
    mockList.mockResolvedValue(current);
    renderHook(() => useReleases(), { wrapper: provider(client) });
    await advance(0);
    expect(mockList).toHaveBeenCalledTimes(1);

    await advance(RELEASES_POLL_MS / 2);
    expect(mockList).toHaveBeenCalledTimes(1);

    await advance(RELEASES_POLL_MS / 2);
    expect(mockList).toHaveBeenCalledTimes(2);

    await advance(RELEASES_POLL_MS);
    expect(mockList).toHaveBeenCalledTimes(3);
  });

  it('stops reading while the page is hidden', async () => {
    await fakeTimers();
    mockList.mockResolvedValue(current);
    renderHook(() => useReleases(), { wrapper: provider(client) });
    await advance(0);
    expect(mockList).toHaveBeenCalledTimes(1);

    hide(true);
    await advance(RELEASES_POLL_MS * 3);

    expect(mockList).toHaveBeenCalledTimes(1);
  });

  it('waits longer between reads while GitHub keeps answering 502', async () => {
    await fakeTimers();
    mockList.mockRejectedValue(gateway);
    const { result, unmount } = renderHook(() => useReleases(), { wrapper: provider(client) });
    await advance(0);
    expect(mockList).toHaveBeenCalledTimes(1);
    expect(result.current.unavailable).toBe(true);

    const [first, second, third] = GATEWAY_BACKOFF_MS;
    await advance(first);
    expect(mockList).toHaveBeenCalledTimes(2);

    await advance(first);
    expect(mockList).toHaveBeenCalledTimes(2);
    await advance(second - first);
    expect(mockList).toHaveBeenCalledTimes(3);

    await advance(second);
    expect(mockList).toHaveBeenCalledTimes(3);
    await advance(third - second);
    expect(mockList).toHaveBeenCalledTimes(4);
    expect(result.current.error).toBeNull();
    unmount();
  });
});
