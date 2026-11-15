import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { HEAD, SOURCE_ID, wireDiff } from '../../../test/pullRequests';
import { PullRequestRefusal } from '../PullRequestRefusal';
import { pullRequestKeys } from '../queryKeys';
import { DiffSchema } from '../schemas';
import type { PullRequestRevision } from '../types';

const mockDiff = mock();

mock.module('../../../api/pullRequests', () => ({
  pullRequestsApi: { diff: mockDiff },
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: { id: 'workspace-1' }, loading: false }),
}));

let usePullRequestDiff: typeof import('./usePullRequestDiff').usePullRequestDiff;

beforeAll(async () => {
  ({ usePullRequestDiff } = await import('./usePullRequestDiff'));
});

afterAll(() => {
  mock.restore();
});

const QUIET = { defaultOptions: { queries: { retry: false, gcTime: 0 } } };

const KEPT = { defaultOptions: { queries: { retry: false } } };

const MOVED = 'd'.repeat(40);

const revision: PullRequestRevision = { source_id: SOURCE_ID, number: 42, head: HEAD };

const diff = DiffSchema.parse(wireDiff);

function provider(client: QueryClient = new QueryClient(QUIET)) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 50));
}

describe('usePullRequestDiff', () => {
  beforeEach(() => {
    mockDiff.mockReset();
  });

  it('reads nothing until the diff is asked for', async () => {
    const { result } = renderHook(() => usePullRequestDiff(revision, false), {
      wrapper: provider(),
    });
    await settle();
    expect(mockDiff).not.toHaveBeenCalled();
    expect(result.current.data).toBeUndefined();
    expect(result.current.loading).toBe(false);
  });

  it('reads the diff at the head it was given', async () => {
    mockDiff.mockResolvedValue(diff);
    const { result } = renderHook(() => usePullRequestDiff(revision, true), {
      wrapper: provider(),
    });
    await waitFor(() => expect(result.current.data).toEqual(diff));
    expect(mockDiff).toHaveBeenCalledWith('workspace-1', revision, HEAD, expect.any(AbortSignal));
  });

  it('reads nothing for a pull request without a head', async () => {
    renderHook(() => usePullRequestDiff({ ...revision, head: null }, true), {
      wrapper: provider(),
    });
    await settle();
    expect(mockDiff).not.toHaveBeenCalled();
  });

  it('reads nothing without a pull request', async () => {
    renderHook(() => usePullRequestDiff(null, true), { wrapper: provider() });
    await settle();
    expect(mockDiff).not.toHaveBeenCalled();
  });

  it('reads again when the head moves', async () => {
    mockDiff.mockResolvedValue(diff);
    const { result, rerender } = renderHook(
      ({ head }) => usePullRequestDiff({ ...revision, head }, true),
      {
        wrapper: provider(),
        initialProps: { head: HEAD },
      }
    );
    await waitFor(() => expect(result.current.data).toEqual(diff));

    const moved = { ...diff, head: MOVED };
    mockDiff.mockResolvedValue(moved);
    rerender({ head: MOVED });

    await waitFor(() => expect(result.current.data).toEqual(moved));
    expect(mockDiff).toHaveBeenLastCalledWith(
      'workspace-1',
      { ...revision, head: MOVED },
      MOVED,
      expect.any(AbortSignal)
    );
  });

  it('keeps the diff of a head without reading it again', async () => {
    mockDiff.mockResolvedValue(diff);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const first = renderHook(() => usePullRequestDiff(revision, true), {
      wrapper: provider(client),
    });
    await waitFor(() => expect(first.result.current.data).toEqual(diff));
    first.unmount();

    const second = renderHook(() => usePullRequestDiff(revision, true), {
      wrapper: provider(client),
    });
    expect(second.result.current.data).toEqual(diff);
    await settle();
    expect(mockDiff).toHaveBeenCalledTimes(1);
  });

  it('reports a head that moved without retrying', async () => {
    mockDiff.mockRejectedValue(
      new PullRequestRefusal({
        error: 'The pull request head moved',
        code: 'head_moved',
        current_head: MOVED,
      })
    );
    const { result } = renderHook(() => usePullRequestDiff(revision, true), {
      wrapper: provider(),
    });
    await waitFor(() => expect(result.current.refusal?.code).toBe('head_moved'));
    expect(result.current.refusal?.currentHead).toBe(MOVED);
    expect(result.current.error).toBe('The pull request head moved');
    expect(mockDiff).toHaveBeenCalledTimes(1);
  });

  it('marks the list out of date when the head moved', async () => {
    mockDiff.mockRejectedValue(
      new PullRequestRefusal({ error: 'Moved', code: 'head_moved', current_head: MOVED })
    );
    const client = new QueryClient(KEPT);
    client.setQueryData(pullRequestKeys.list('workspace-1'), {});
    renderHook(() => usePullRequestDiff(revision, true), { wrapper: provider(client) });

    await waitFor(() =>
      expect(
        client.getQueryCache().find({ queryKey: pullRequestKeys.list('workspace-1') })?.state
          .isInvalidated
      ).toBe(true)
    );
  });

  it('leaves the list alone for any other failure', async () => {
    mockDiff.mockRejectedValue(new PullRequestRefusal({ error: 'Conflict', code: 'conflict' }));
    const client = new QueryClient(KEPT);
    client.setQueryData(pullRequestKeys.list('workspace-1'), {});
    const { result } = renderHook(() => usePullRequestDiff(revision, true), {
      wrapper: provider(client),
    });

    await waitFor(() => expect(result.current.refusal?.code).toBe('conflict'));
    expect(
      client.getQueryCache().find({ queryKey: pullRequestKeys.list('workspace-1') })?.state
        .isInvalidated
    ).toBe(false);
  });
});
