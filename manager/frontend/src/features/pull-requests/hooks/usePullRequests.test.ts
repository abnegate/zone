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
import { wirePullRequests } from '../../../test/pullRequests';
import { fakeTimers } from '../../../test/timers';
import { GATEWAY_BACKOFF_MS } from '../../github';
import { PullRequestsResponseSchema } from '../schemas';
import type { PullRequestsResponse } from '../types';

const mockList = mock();

mock.module('../../../api/pullRequests', () => ({
  pullRequestsApi: { list: mockList },
}));

let workspace: { id: string } | null = { id: 'workspace-1' };

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: workspace, loading: false }),
}));

let usePullRequests: typeof import('./usePullRequests').usePullRequests;
let LIST_POLL_MS: number;
let floorEnd: typeof import('./usePullRequests').floorEnd;

beforeAll(async () => {
  ({ usePullRequests, LIST_POLL_MS, floorEnd } = await import('./usePullRequests'));
});

afterAll(() => {
  mock.restore();
});

const QUIET = { defaultOptions: { queries: { retry: false, gcTime: 0 } } };

const LONG_AGO = '2026-01-01T00:00:00Z';

function listing(changes: Partial<PullRequestsResponse> = {}): PullRequestsResponse {
  return { ...PullRequestsResponseSchema.parse(wirePullRequests), ...changes };
}

function provider(client: QueryClient = new QueryClient(QUIET)) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

function settle(milliseconds = 50): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

let faked = false;

async function fake(): Promise<void> {
  await fakeTimers();
  faked = true;
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

describe('usePullRequests', () => {
  beforeEach(() => {
    mockList.mockReset();
    workspace = { id: 'workspace-1' };
  });

  afterEach(async () => {
    if (faked) {
      await advance(0);
      faked = false;
    }
    vi.useRealTimers();
    hide(false);
  });

  it('reads the open pull requests of the workspace', async () => {
    const listed = listing({ generated_at: LONG_AGO, warnings: ['acme/api: rate limited'] });
    mockList.mockResolvedValue(listed);
    const { result } = renderHook(() => usePullRequests(), { wrapper: provider() });

    await waitFor(() => expect(result.current.pullRequests).toEqual(listed.pull_requests));
    expect(mockList).toHaveBeenCalledWith('workspace-1', false, expect.any(AbortSignal));
    expect(result.current.sources).toEqual(listed.sources);
    expect(result.current.warnings).toEqual(['acme/api: rate limited']);
    expect(result.current.generatedAt).toBe(LONG_AGO);
    expect(result.current.stale).toBe(false);
    expect(result.current.partial).toBe(false);
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('reports a stale or partial listing', async () => {
    mockList.mockResolvedValue(listing({ stale: true, partial: true }));
    const { result } = renderHook(() => usePullRequests(), { wrapper: provider() });
    await waitFor(() => expect(result.current.stale).toBe(true));
    expect(result.current.partial).toBe(true);
  });

  it('reads nothing without a workspace', async () => {
    workspace = null;
    const { result } = renderHook(() => usePullRequests(), { wrapper: provider() });
    await settle();
    expect(mockList).not.toHaveBeenCalled();
    expect(result.current.pullRequests).toEqual([]);
    expect(result.current.loading).toBe(false);
    expect(result.current.refreshable).toBe(false);
  });

  it('says GitHub is unreachable while it waits to retry a 502', async () => {
    mockList.mockRejectedValue(new ApiError('Bad gateway', HttpStatus.BadGateway));
    const { result, unmount } = renderHook(() => usePullRequests(), { wrapper: provider() });

    await waitFor(() => expect(result.current.unavailable).toBe(true));
    expect(mockList).toHaveBeenCalledTimes(1);
    expect(result.current.loading).toBe(true);
    unmount();
  });

  it('reports a failed read', async () => {
    mockList.mockRejectedValue(new ApiError('Workspace not found', HttpStatus.NotFound));
    const { result } = renderHook(() => usePullRequests(), { wrapper: provider() });
    await waitFor(() => expect(result.current.error).toBe('Workspace not found'));
    expect(result.current.unavailable).toBe(false);
    expect(mockList).toHaveBeenCalledTimes(1);
  });

  it('reports an unreadable source configuration once, without calling GitHub unreachable', async () => {
    mockList.mockRejectedValue(
      new ApiError('The GitHub source configuration is invalid.', HttpStatus.Conflict)
    );
    const { result } = renderHook(() => usePullRequests(), { wrapper: provider() });
    await waitFor(() =>
      expect(result.current.error).toBe('The GitHub source configuration is invalid.')
    );
    expect(result.current.unavailable).toBe(false);
    expect(mockList).toHaveBeenCalledTimes(1);
  });

  it('refreshes from GitHub and replaces the listing', async () => {
    const listed = listing({ generated_at: LONG_AGO });
    const refreshed = listing({
      generated_at: LONG_AGO,
      counts: { ready: 1, in_progress: 0, not_ready: 0, total: 1 },
      pull_requests: [{ ...listed.pull_requests[0], readiness: 'ready', blockers: [] }],
    });
    mockList.mockImplementation((_workspaceId: string, refresh: boolean) =>
      Promise.resolve(refresh ? refreshed : listed)
    );
    const { result } = renderHook(() => usePullRequests(), { wrapper: provider() });
    await waitFor(() => expect(result.current.refreshable).toBe(true));

    act(() => result.current.refresh());

    await waitFor(() => expect(result.current.pullRequests[0].readiness).toBe('ready'));
    expect(mockList).toHaveBeenCalledWith('workspace-1', true);
    expect(mockList).toHaveBeenCalledTimes(2);
    expect(result.current.pullRequests[0].readiness).toBe('ready');
    expect(result.current.refreshing).toBe(false);
  });

  it('holds refresh back while the listing is inside the refresh floor', async () => {
    mockList.mockResolvedValue(
      listing({ generated_at: new Date().toISOString(), refresh_floor_secs: 60 })
    );
    const { result } = renderHook(() => usePullRequests(), { wrapper: provider() });
    await waitFor(() => expect(result.current.pullRequests).toHaveLength(1));

    expect(result.current.floored).toBe(true);
    expect(result.current.refreshable).toBe(false);
    act(() => result.current.refresh());
    await settle();
    expect(mockList).toHaveBeenCalledTimes(1);
  });

  it('allows refresh again once the floor has passed', async () => {
    mockList.mockResolvedValue(
      listing({ generated_at: new Date().toISOString(), refresh_floor_secs: 0.2 })
    );
    const { result } = renderHook(() => usePullRequests(), { wrapper: provider() });
    await waitFor(() => expect(result.current.floored).toBe(true));
    await waitFor(() => expect(result.current.refreshable).toBe(true), { timeout: 1_000 });
    expect(result.current.floored).toBe(false);
  });

  it('says GitHub is unreachable when a refresh answers 502', async () => {
    mockList.mockImplementation((_workspaceId: string, refresh: boolean) =>
      refresh
        ? Promise.reject(new ApiError('Bad gateway', HttpStatus.BadGateway))
        : Promise.resolve(listing({ generated_at: LONG_AGO }))
    );
    const { result } = renderHook(() => usePullRequests(), { wrapper: provider() });
    await waitFor(() => expect(result.current.refreshable).toBe(true));

    act(() => result.current.refresh());

    await waitFor(() => expect(result.current.unavailable).toBe(true));
    expect(result.current.error).toBe('Bad gateway');
    expect(result.current.pullRequests).toHaveLength(1);
  });

  it('clears a failed refresh once a later read succeeds', async () => {
    let refreshes = 0;
    mockList.mockImplementation((_workspaceId: string, refresh: boolean) => {
      if (!refresh) return Promise.resolve(listing({ generated_at: LONG_AGO }));
      refreshes += 1;
      return Promise.reject(new ApiError('Rate limited', HttpStatus.BadGateway));
    });
    const client = new QueryClient(QUIET);
    const { result } = renderHook(() => usePullRequests(), { wrapper: provider(client) });
    await waitFor(() => expect(result.current.refreshable).toBe(true));

    act(() => result.current.refresh());
    await waitFor(() => expect(result.current.error).toBe('Rate limited'));

    act(() => {
      void client.refetchQueries();
    });

    await waitFor(() => expect(result.current.error).toBeNull());
    expect(refreshes).toBe(1);
  });

  it('reads the list again every 30 seconds', async () => {
    await fake();
    mockList.mockResolvedValue(listing());
    const { unmount } = renderHook(() => usePullRequests(), { wrapper: provider() });
    await advance(0);
    expect(mockList).toHaveBeenCalledTimes(1);

    await advance(LIST_POLL_MS / 2);
    expect(mockList).toHaveBeenCalledTimes(1);

    await advance(LIST_POLL_MS / 2);
    expect(mockList).toHaveBeenCalledTimes(2);

    await advance(LIST_POLL_MS);
    expect(mockList).toHaveBeenCalledTimes(3);
    unmount();
  });

  it('stops reading while the page is hidden and reads again when it shows', async () => {
    await fake();
    mockList.mockResolvedValue(listing());
    const { unmount } = renderHook(() => usePullRequests(), { wrapper: provider() });
    await advance(0);
    expect(mockList).toHaveBeenCalledTimes(1);

    hide(true);
    await advance(LIST_POLL_MS * 3);
    expect(mockList).toHaveBeenCalledTimes(1);

    hide(false);
    await advance(0);
    expect(mockList).toHaveBeenCalledTimes(2);
    unmount();
  });

  it('waits longer between reads while GitHub keeps answering 502', async () => {
    await fake();
    mockList.mockRejectedValue(new ApiError('Bad gateway', HttpStatus.BadGateway));
    const { result, unmount } = renderHook(() => usePullRequests(), { wrapper: provider() });
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
    unmount();
  });
});

describe('floorEnd', () => {
  const generatedAt = Date.parse('2026-11-13T10:00:00Z');

  it('ends the floor its length after the listing was generated', () => {
    const listed = listing({ generated_at: '2026-11-13T10:00:00Z', refresh_floor_secs: 10 });
    expect(floorEnd(listed, generatedAt + 5_000)).toBe(generatedAt + 10_000);
  });

  it('never runs longer than the floor from when the listing arrived', () => {
    const listed = listing({ generated_at: '2026-11-13T10:00:00Z', refresh_floor_secs: 10 });
    expect(floorEnd(listed, generatedAt - 60_000)).toBe(generatedAt - 50_000);
  });

  it('times the floor from arrival when the generation time is unreadable', () => {
    const listed = listing({ generated_at: 'soon', refresh_floor_secs: 10 });
    expect(floorEnd(listed, generatedAt)).toBe(generatedAt + 10_000);
  });
});
