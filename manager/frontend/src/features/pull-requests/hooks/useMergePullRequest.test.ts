import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { HEAD, SOURCE_ID, wireMerged, wirePullRequests } from '../../../test/pullRequests';
import { releasesKey } from '../../releases';
import { PullRequestRefusal } from '../PullRequestRefusal';
import { pullRequestKeys } from '../queryKeys';
import { MergedSchema } from '../schemas';
import type { PullRequestRevision } from '../types';

const mockMerge = mock();

mock.module('../../../api/pullRequests', () => ({
  pullRequestsApi: { merge: mockMerge },
}));

let workspace: { id: string } | null = { id: 'workspace-1' };

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: workspace, loading: false }),
}));

let useMergePullRequest: typeof import('./useMergePullRequest').useMergePullRequest;
let NO_HEAD: string;
let MERGE_OUTCOME_UNKNOWN: string;

beforeAll(async () => {
  ({ useMergePullRequest, NO_HEAD, MERGE_OUTCOME_UNKNOWN } = await import('./useMergePullRequest'));
});

afterAll(() => {
  mock.restore();
});

const revision: PullRequestRevision = { source_id: SOURCE_ID, number: 42, head: HEAD };

const merged = MergedSchema.parse(wireMerged);

const MOVED = 'd'.repeat(40);

function seeded(): QueryClient {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(pullRequestKeys.list('workspace-1'), wirePullRequests);
  client.setQueryData(pullRequestKeys.detail('workspace-1', revision), {});
  client.setQueryData(pullRequestKeys.diff('workspace-1', revision), {});
  client.setQueryData(pullRequestKeys.commits('workspace-1', revision), {});
  client.setQueryData(pullRequestKeys.commit('workspace-1', revision, HEAD), {});
  client.setQueryData(pullRequestKeys.log('workspace-1', revision, 9001), {});
  client.setQueryData(releasesKey('workspace-1'), {});
  client.setQueryData(pullRequestKeys.list('workspace-2'), wirePullRequests);
  client.setQueryData(releasesKey('workspace-2'), {});
  return client;
}

function invalidated(client: QueryClient, queryKey: readonly unknown[]): boolean | undefined {
  return client.getQueryCache().find({ queryKey, exact: true })?.state.isInvalidated;
}

function provider(client: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

describe('useMergePullRequest', () => {
  beforeEach(() => {
    mockMerge.mockReset();
    workspace = { id: 'workspace-1' };
  });

  it('merges at the head the console last saw, with the chosen method', async () => {
    mockMerge.mockResolvedValue(merged);
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(seeded()) });

    let answer: unknown;
    await act(async () => {
      answer = await result.current.merge(revision, 'squash');
    });

    expect(answer).toEqual(merged);
    expect(mockMerge).toHaveBeenCalledWith('workspace-1', revision, {
      expected_head: HEAD,
      method: 'squash',
    });
    expect(result.current.error).toBeNull();
    expect(result.current.merging).toBe(false);
  });

  it('leaves the method to the server when none is chosen', async () => {
    mockMerge.mockResolvedValue(merged);
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(seeded()) });
    await act(() => result.current.merge(revision));
    expect(mockMerge.mock.calls[0][2]).toEqual({ expected_head: HEAD, method: undefined });
  });

  it('marks the workspace pull request listings and releases out of date after a merge', async () => {
    mockMerge.mockResolvedValue(merged);
    const client = seeded();
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(client) });

    await act(() => result.current.merge(revision));

    await waitFor(() =>
      expect(invalidated(client, pullRequestKeys.list('workspace-1'))).toBe(true)
    );
    expect(invalidated(client, pullRequestKeys.detail('workspace-1', revision))).toBe(true);
    for (const artifact of [
      pullRequestKeys.diff('workspace-1', revision),
      pullRequestKeys.commits('workspace-1', revision),
      pullRequestKeys.commit('workspace-1', revision, HEAD),
      pullRequestKeys.log('workspace-1', revision, 9001),
    ]) {
      expect(invalidated(client, artifact)).toBe(false);
    }
    expect(invalidated(client, releasesKey('workspace-1'))).toBe(true);
    expect(invalidated(client, pullRequestKeys.list('workspace-2'))).toBe(false);
    expect(invalidated(client, releasesKey('workspace-2'))).toBe(false);
  });

  it('reports a refusal and re-reads the pull requests, but not the releases', async () => {
    mockMerge.mockRejectedValue(
      new PullRequestRefusal({
        error: 'The pull request head moved',
        code: 'head_moved',
        current_head: MOVED,
      })
    );
    const client = seeded();
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(client) });

    await act(async () => {
      await expect(result.current.merge(revision)).rejects.toBeInstanceOf(PullRequestRefusal);
    });

    await waitFor(() => expect(result.current.refusal?.code).toBe('head_moved'));
    expect(result.current.refusal?.currentHead).toBe(MOVED);
    expect(result.current.error).toBe('The pull request head moved');
    expect(invalidated(client, pullRequestKeys.list('workspace-1'))).toBe(true);
    expect(invalidated(client, releasesKey('workspace-1'))).toBe(false);
  });

  it.each([
    ['merged', 'The pull request was already merged.'],
    ['closed', 'The pull request is closed. Reopen it to merge it.'],
  ] as const)('settles a pull request found %s and re-reads the list', async (code, message) => {
    mockMerge.mockRejectedValue(new PullRequestRefusal({ error: message, code }));
    const client = seeded();
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(client) });

    await act(async () => {
      await result.current.merge(revision).catch(() => undefined);
    });

    await waitFor(() => expect(result.current.settled).toBe(true));
    expect(result.current.error).toBe(message);
    expect(result.current.unknown).toBe(false);
    expect(invalidated(client, pullRequestKeys.list('workspace-1'))).toBe(true);
    expect(invalidated(client, releasesKey('workspace-1'))).toBe(false);
  });

  it('does not call a refusal that may pass later settled', async () => {
    mockMerge.mockRejectedValue(new PullRequestRefusal({ error: 'Conflict', code: 'conflict' }));
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(seeded()) });
    await act(async () => {
      await result.current.merge(revision).catch(() => undefined);
    });
    await waitFor(() => expect(result.current.refusal?.code).toBe('conflict'));
    expect(result.current.settled).toBe(false);
  });

  it('asks for GitHub to be checked and re-reads everything when the outcome is unknown', async () => {
    mockMerge.mockRejectedValue(
      new ApiError(
        'Failed to merge pull request: The merge stopped before it reported back',
        HttpStatus.InternalServerError
      )
    );
    const client = seeded();
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(client) });

    await act(async () => {
      await result.current.merge(revision).catch(() => undefined);
    });

    await waitFor(() => expect(result.current.unknown).toBe(true));
    expect(result.current.error).toBe(MERGE_OUTCOME_UNKNOWN);
    expect(result.current.settled).toBe(false);
    expect(invalidated(client, pullRequestKeys.list('workspace-1'))).toBe(true);
    expect(invalidated(client, releasesKey('workspace-1'))).toBe(true);
  });

  it('reports a missing permission as the server put it', async () => {
    const message = 'Failed to merge pull request: Missing permission: pull_requests:update';
    mockMerge.mockRejectedValue(new ApiError(message, HttpStatus.Forbidden));
    const client = seeded();
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(client) });

    await act(async () => {
      await result.current.merge(revision).catch(() => undefined);
    });

    await waitFor(() => expect(result.current.error).toBe(message));
    expect(result.current.unknown).toBe(false);
    expect(invalidated(client, releasesKey('workspace-1'))).toBe(false);
  });

  it('clears a refusal on reset', async () => {
    mockMerge.mockRejectedValue(new PullRequestRefusal({ error: 'Not ready', code: 'not_ready' }));
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(seeded()) });
    await act(async () => {
      await result.current.merge(revision).catch(() => undefined);
    });
    await waitFor(() => expect(result.current.refusal?.code).toBe('not_ready'));

    act(() => result.current.reset());

    await waitFor(() => expect(result.current.refusal).toBeNull());
    expect(result.current.error).toBeNull();
  });

  it('refuses to merge a pull request without a head', async () => {
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(seeded()) });
    await act(async () => {
      await expect(result.current.merge({ ...revision, head: null })).rejects.toThrow(NO_HEAD);
    });
    expect(mockMerge).not.toHaveBeenCalled();
  });

  it('refuses to merge without a workspace', async () => {
    workspace = null;
    const { result } = renderHook(() => useMergePullRequest(), { wrapper: provider(seeded()) });
    await act(async () => {
      await expect(result.current.merge(revision)).rejects.toThrow('No workspace selected');
    });
    expect(mockMerge).not.toHaveBeenCalled();
  });
});
