import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { HEAD, SOURCE_ID, wireLog } from '../../../test/pullRequests';
import { PullRequestRefusal } from '../PullRequestRefusal';
import { pullRequestKeys } from '../queryKeys';
import { CheckLogSchema } from '../schemas';
import type { PullRequestRevision } from '../types';

const mockJobLog = mock();

mock.module('../../../api/pullRequests', () => ({
  pullRequestsApi: { jobLog: mockJobLog },
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: { id: 'workspace-1' }, loading: false }),
}));

let useCheckLog: typeof import('./useCheckLog').useCheckLog;

beforeAll(async () => {
  ({ useCheckLog } = await import('./useCheckLog'));
});

afterAll(() => {
  mock.restore();
});

const QUIET = { defaultOptions: { queries: { retry: false, gcTime: 0 } } };

const revision: PullRequestRevision = { source_id: SOURCE_ID, number: 42, head: HEAD };

const log = CheckLogSchema.parse(wireLog);

function provider() {
  const client = new QueryClient(QUIET);
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 50));
}

describe('useCheckLog', () => {
  beforeEach(() => {
    mockJobLog.mockReset();
  });

  it('reads nothing until a log is asked for', async () => {
    const { result } = renderHook(() => useCheckLog(revision, null), { wrapper: provider() });
    await settle();
    expect(mockJobLog).not.toHaveBeenCalled();
    expect(result.current.data).toBeUndefined();
  });

  it('reads the log of the job at the head it was given', async () => {
    mockJobLog.mockResolvedValue(log);
    const { result } = renderHook(() => useCheckLog(revision, 9001), { wrapper: provider() });
    await waitFor(() => expect(result.current.data).toEqual(log));
    expect(mockJobLog).toHaveBeenCalledWith(
      'workspace-1',
      revision,
      9001,
      HEAD,
      expect.any(AbortSignal)
    );
  });

  it('reads nothing for a pull request without a head', async () => {
    renderHook(() => useCheckLog({ ...revision, head: null }, 9001), { wrapper: provider() });
    await settle();
    expect(mockJobLog).not.toHaveBeenCalled();
  });

  it('reports a log that is gone without retrying', async () => {
    mockJobLog.mockRejectedValue(new ApiError('Job not found', HttpStatus.NotFound));
    const { result } = renderHook(() => useCheckLog(revision, 9001), { wrapper: provider() });
    await waitFor(() => expect(result.current.error).toBe('Job not found'));
    expect(mockJobLog).toHaveBeenCalledTimes(1);
  });

  it('marks the list out of date when the head moved', async () => {
    mockJobLog.mockRejectedValue(
      new PullRequestRefusal({ error: 'Moved', code: 'head_moved', current_head: 'd'.repeat(40) })
    );
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryData(pullRequestKeys.list('workspace-1'), {});
    renderHook(() => useCheckLog(revision, 9001), {
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
