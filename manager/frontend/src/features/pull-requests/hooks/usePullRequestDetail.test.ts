import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { HEAD, SOURCE_ID, wireDetail } from '../../../test/pullRequests';
import { PullRequestDetailSchema } from '../schemas';
import type { PullRequestRevision } from '../types';

const mockDetail = mock();

mock.module('../../../api/pullRequests', () => ({
  pullRequestsApi: { detail: mockDetail },
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: { id: 'workspace-1' }, loading: false }),
}));

let usePullRequestDetail: typeof import('./usePullRequestDetail').usePullRequestDetail;

beforeAll(async () => {
  ({ usePullRequestDetail } = await import('./usePullRequestDetail'));
});

afterAll(() => {
  mock.restore();
});

const QUIET = { defaultOptions: { queries: { retry: false, gcTime: 0 } } };

const revision: PullRequestRevision = { source_id: SOURCE_ID, number: 42, head: HEAD };

const detail = PullRequestDetailSchema.parse(wireDetail);

function provider() {
  const client = new QueryClient(QUIET);
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 50));
}

describe('usePullRequestDetail', () => {
  beforeEach(() => {
    mockDetail.mockReset();
  });

  it('reads nothing while no pull request is selected', async () => {
    const { result } = renderHook(() => usePullRequestDetail(null), { wrapper: provider() });
    await settle();
    expect(mockDetail).not.toHaveBeenCalled();
    expect(result.current.loading).toBe(false);
  });

  it('reads nothing while it is not wanted', async () => {
    renderHook(() => usePullRequestDetail(revision, false), { wrapper: provider() });
    await settle();
    expect(mockDetail).not.toHaveBeenCalled();
  });

  it('reads the threads and bot comments of the selected pull request', async () => {
    mockDetail.mockResolvedValue(detail);
    const { result } = renderHook(() => usePullRequestDetail(revision), { wrapper: provider() });
    await waitFor(() => expect(result.current.data).toEqual(detail));
    expect(mockDetail).toHaveBeenCalledWith('workspace-1', revision, expect.any(AbortSignal));
  });

  it('reads a pull request without a head', async () => {
    mockDetail.mockResolvedValue(detail);
    const headless = { ...revision, head: null };
    const { result } = renderHook(() => usePullRequestDetail(headless), { wrapper: provider() });
    await waitFor(() => expect(result.current.data).toEqual(detail));
  });

  it('reads again when the head moves', async () => {
    mockDetail.mockResolvedValue(detail);
    const { rerender } = renderHook(({ head }) => usePullRequestDetail({ ...revision, head }), {
      wrapper: provider(),
      initialProps: { head: HEAD },
    });
    await waitFor(() => expect(mockDetail).toHaveBeenCalledTimes(1));
    rerender({ head: 'd'.repeat(40) });
    await waitFor(() => expect(mockDetail).toHaveBeenCalledTimes(2));
  });

  it('reports a pull request that is gone without retrying', async () => {
    mockDetail.mockRejectedValue(new ApiError('Pull request not found', HttpStatus.NotFound));
    const { result } = renderHook(() => usePullRequestDetail(revision), { wrapper: provider() });
    await waitFor(() => expect(result.current.error).toBe('Pull request not found'));
    expect(result.current.refusal).toBeNull();
    expect(mockDetail).toHaveBeenCalledTimes(1);
  });
});
