import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { HEAD, SOURCE_ID, wireCommits, wireDiff } from '../../../test/pullRequests';
import { PullRequestRefusal } from '../PullRequestRefusal';
import { pullRequestKeys } from '../queryKeys';
import { CommitListSchema, DiffSchema } from '../schemas';
import type { PullRequestRevision } from '../types';

const mockCommits = mock();
const mockCommitDiff = mock();

mock.module('../../../api/pullRequests', () => ({
  pullRequestsApi: { commits: mockCommits, commitDiff: mockCommitDiff },
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: { id: 'workspace-1' }, loading: false }),
}));

let usePullRequestCommits: typeof import('./usePullRequestCommits').usePullRequestCommits;
let useCommitDiff: typeof import('./usePullRequestCommits').useCommitDiff;

beforeAll(async () => {
  ({ usePullRequestCommits, useCommitDiff } = await import('./usePullRequestCommits'));
});

afterAll(() => {
  mock.restore();
});

const QUIET = { defaultOptions: { queries: { retry: false, gcTime: 0 } } };

const SHA = 'c'.repeat(40);

const revision: PullRequestRevision = { source_id: SOURCE_ID, number: 42, head: HEAD };

const commits = CommitListSchema.parse(wireCommits);

const diff = DiffSchema.parse(wireDiff);

function provider() {
  const client = new QueryClient(QUIET);
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 50));
}

describe('usePullRequestCommits', () => {
  beforeEach(() => {
    mockCommits.mockReset();
  });

  it('reads nothing until the commits are asked for', async () => {
    renderHook(() => usePullRequestCommits(revision, false), { wrapper: provider() });
    await settle();
    expect(mockCommits).not.toHaveBeenCalled();
  });

  it('reads the commits at the head it was given', async () => {
    mockCommits.mockResolvedValue(commits);
    const { result } = renderHook(() => usePullRequestCommits(revision, true), {
      wrapper: provider(),
    });
    await waitFor(() => expect(result.current.data).toEqual(commits));
    expect(mockCommits).toHaveBeenCalledWith(
      'workspace-1',
      revision,
      HEAD,
      expect.any(AbortSignal)
    );
  });

  it('reads nothing for a pull request without a head', async () => {
    renderHook(() => usePullRequestCommits({ ...revision, head: null }, true), {
      wrapper: provider(),
    });
    await settle();
    expect(mockCommits).not.toHaveBeenCalled();
  });
});

describe('useCommitDiff', () => {
  beforeEach(() => {
    mockCommitDiff.mockReset();
  });

  it('reads nothing until a commit is chosen', async () => {
    const { result } = renderHook(() => useCommitDiff(revision, null), { wrapper: provider() });
    await settle();
    expect(mockCommitDiff).not.toHaveBeenCalled();
    expect(result.current.loading).toBe(false);
  });

  it('reads the diff of the chosen commit at the head it was given', async () => {
    mockCommitDiff.mockResolvedValue(diff);
    const { result } = renderHook(() => useCommitDiff(revision, SHA), { wrapper: provider() });
    await waitFor(() => expect(result.current.data).toEqual(diff));
    expect(mockCommitDiff).toHaveBeenCalledWith(
      'workspace-1',
      revision,
      SHA,
      HEAD,
      expect.any(AbortSignal)
    );
  });

  it('reads again when another commit is chosen', async () => {
    mockCommitDiff.mockResolvedValue(diff);
    const { rerender } = renderHook(({ sha }) => useCommitDiff(revision, sha), {
      wrapper: provider(),
      initialProps: { sha: SHA },
    });
    await waitFor(() => expect(mockCommitDiff).toHaveBeenCalledTimes(1));
    rerender({ sha: 'e'.repeat(40) });
    await waitFor(() => expect(mockCommitDiff).toHaveBeenCalledTimes(2));
    expect(mockCommitDiff.mock.calls[1][2]).toBe('e'.repeat(40));
  });

  it('marks the list out of date when the head moved under the commits', async () => {
    mockCommits.mockRejectedValue(
      new PullRequestRefusal({ error: 'Moved', code: 'head_moved', current_head: 'd'.repeat(40) })
    );
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryData(pullRequestKeys.list('workspace-1'), {});
    renderHook(() => usePullRequestCommits(revision, true), {
      wrapper: ({ children }: { children: ReactNode }) =>
        createElement(QueryClientProvider, { client }, children),
    });

    await waitFor(() =>
      expect(
        client.getQueryCache().find({ queryKey: pullRequestKeys.list('workspace-1') })?.state
          .isInvalidated
      ).toBe(true)
    );
  });
});
