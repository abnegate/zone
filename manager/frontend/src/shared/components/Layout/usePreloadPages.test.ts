import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { PullRequestsResponseSchema } from '../../../features/pull-requests/schemas';
import type { Release } from '../../../features/releases/types';
import { wirePullRequests } from '../../../test/pullRequests';
import { listing, pipelineRun, release } from '../../../test/releases';
import { PERMISSIONS } from '../../types/permissions';

const mockPullRequests = mock();
const mockReleases = mock();

mock.module('../../../api/pullRequests', () => ({
  pullRequestsApi: { list: mockPullRequests },
}));

mock.module('../../../api/releases', () => ({
  releasesApi: { list: mockReleases },
}));

let workspace: { id: string } | null = { id: 'workspace-1' };

mock.module('../../context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: workspace, loading: false }),
}));

let granted: readonly string[] = [];

mock.module('../../../features/auth/context', () => ({
  useAuth: () => ({ hasPermission: (permission: string) => granted.includes(permission) }),
}));

let pullRequestKeys: typeof import('../../../features/pull-requests/queryKeys').pullRequestKeys;
let releaseWindowKey: typeof import('../../../features/releases/hooks/keys').releaseWindowKey;
let usePreloadPages: typeof import('./usePreloadPages').usePreloadPages;
let usePullRequests: typeof import('../../../features/pull-requests/hooks/usePullRequests').usePullRequests;
let useReleases: typeof import('../../../features/releases/hooks/useReleases').useReleases;

beforeAll(async () => {
  ({ usePreloadPages } = await import('./usePreloadPages'));
  ({ pullRequestKeys } = await import('../../../features/pull-requests/queryKeys'));
  ({ releaseWindowKey } = await import('../../../features/releases/hooks/keys'));
  ({ usePullRequests } = await import('../../../features/pull-requests/hooks/usePullRequests'));
  ({ useReleases } = await import('../../../features/releases/hooks/useReleases'));
});

afterAll(() => {
  mock.restore();
});

function checked(subject: Release, at: string, state: Release['state']): Release {
  return {
    ...subject,
    state,
    succeeded: state === 'succeeded',
    complete: state === 'succeeded',
    pipeline: { ...subject.pipeline, runs: [pipelineRun({ state })], checked_at: at },
  };
}

const pullRequests = PullRequestsResponseSchema.parse(wirePullRequests);
const releases = listing([release()]);

function provider(client: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

function quiet(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

beforeEach(() => {
  workspace = { id: 'workspace-1' };
  granted = [PERMISSIONS.PULL_REQUESTS.READ, PERMISSIONS.RELEASES.READ];
  mockPullRequests.mockReset();
  mockReleases.mockReset();
  mockPullRequests.mockResolvedValue(pullRequests);
  mockReleases.mockResolvedValue(releases);
});

describe('usePreloadPages', () => {
  it('fills the caches the pull requests and releases pages read', async () => {
    const client = quiet();
    renderHook(() => usePreloadPages(), { wrapper: provider(client) });

    await waitFor(() => {
      expect(mockPullRequests).toHaveBeenCalledTimes(1);
      expect(mockReleases).toHaveBeenCalledTimes(1);
    });
    expect(mockPullRequests.mock.calls[0]?.slice(0, 2)).toEqual(['workspace-1', false]);
    expect(mockReleases.mock.calls[0]?.slice(0, 2)).toEqual(['workspace-1', { days: undefined }]);

    await waitFor(() => {
      const pages = renderHook(() => [usePullRequests(), useReleases()] as const, {
        wrapper: provider(client),
      });
      const [listed, released] = pages.result.current;
      expect(listed.loading).toBe(false);
      expect(listed.pullRequests).toEqual(pullRequests.pull_requests);
      expect(released.loading).toBe(false);
      expect(released.releases).toEqual(releases.releases);
      pages.unmount();
    });
  });

  it('keeps preloaded listings after their last reader leaves', async () => {
    const client = quiet();
    const preloader = renderHook(() => usePreloadPages(), { wrapper: provider(client) });
    await waitFor(() => expect(mockReleases).toHaveBeenCalledTimes(1));
    preloader.unmount();

    const listings = client.getQueryCache().getAll();
    expect(listings).toHaveLength(2);
    for (const query of listings) {
      expect(query.gcTime).toBe(Number.POSITIVE_INFINITY);
    }
  });

  it('finishes a preload the page joined and left', async () => {
    let answer: (value: typeof pullRequests) => void = () => {};
    mockPullRequests.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    const client = quiet();
    renderHook(() => usePreloadPages(), { wrapper: provider(client) });
    await waitFor(() => expect(mockPullRequests).toHaveBeenCalledTimes(1));

    const page = renderHook(() => usePullRequests(), { wrapper: provider(client) });
    page.unmount();
    answer(pullRequests);

    await waitFor(() =>
      expect(client.getQueryData(pullRequestKeys.list('workspace-1'))).toEqual(pullRequests)
    );
    expect(mockPullRequests).toHaveBeenCalledTimes(1);
  });

  it('keeps a pipeline verdict newer than the preloaded listing', async () => {
    const client = quiet();
    const running = checked(release(), '2026-01-01T00:00:00Z', 'running');
    const succeeded = checked(release(), '2026-01-01T00:05:00Z', 'succeeded');
    client.setQueryData(releaseWindowKey('workspace-1'), listing([succeeded]));
    mockReleases.mockResolvedValue(listing([running]));

    renderHook(() => usePreloadPages(), { wrapper: provider(client) });

    await waitFor(() => expect(mockReleases).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(client.isFetching()).toBe(0));
    const cached = client.getQueryData<ReturnType<typeof listing>>(releaseWindowKey('workspace-1'));
    expect(cached?.releases[0]?.state).toBe('succeeded');
  });

  it('loads nothing the reader may not see', async () => {
    granted = [PERMISSIONS.RELEASES.READ];
    const client = quiet();
    renderHook(() => usePreloadPages(), { wrapper: provider(client) });

    await waitFor(() => expect(mockReleases).toHaveBeenCalledTimes(1));
    expect(mockPullRequests).not.toHaveBeenCalled();
  });

  it('waits for a workspace', () => {
    workspace = null;
    const client = quiet();
    renderHook(() => usePreloadPages(), { wrapper: provider(client) });

    expect(mockPullRequests).not.toHaveBeenCalled();
    expect(mockReleases).not.toHaveBeenCalled();
  });
});
