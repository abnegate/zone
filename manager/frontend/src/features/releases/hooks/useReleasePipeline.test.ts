import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { listing, pipelineRun, release, SOURCE_ID, verdict } from '../../../test/releases';
import { retryDelay, retryPolled } from '../../github';
import type { Release, ReleasesResponse } from '../types';

const mockPipeline = mock();

mock.module('../../../api/releases', () => ({
  releasesApi: { pipeline: mockPipeline },
}));

let workspace: { id: string } | null = { id: 'workspace-1' };

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: workspace, loading: false }),
}));

let useReleasePipeline: typeof import('./useReleasePipeline').useReleasePipeline;
let PIPELINE_POLL_MS: number;
let keys: typeof import('./keys');

beforeAll(async () => {
  ({ useReleasePipeline, PIPELINE_POLL_MS } = await import('./useReleasePipeline'));
  keys = await import('./keys');
});

afterAll(() => {
  mock.restore();
});

const MINUTE = 60_000;
const QUIET = { defaultOptions: { queries: { retry: false } } };

function provider(client: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

function ago(minutes: number): string {
  return new Date(Date.now() - minutes * MINUTE).toISOString();
}

function pending(minutes: number, checked = 10): Release {
  return release({
    published_at: ago(minutes),
    checked_at: ago(checked),
    lookup: 'pending',
    runs: [],
    state: 'unknown',
    succeeded: false,
    complete: false,
    blockers: [{ code: 'lookup_pending', detail: 'No run yet' }],
  });
}

function settled(subject: Release): Release {
  return release({
    published_at: subject.published_at,
    runs: [pipelineRun()],
    checked_at: new Date().toISOString(),
  });
}

function interval(client: QueryClient, subject: Release): number | false | undefined {
  const query = client.getQueryCache().find({
    queryKey: keys.releasePipelineKey('workspace-1', subject.source_id, subject.id),
  });
  if (!query) throw new Error('The pipeline was never queried');
  const { refetchInterval } = query.observers[0].options;
  return typeof refetchInterval === 'function' ? refetchInterval(query) : refetchInterval;
}

async function idle(): Promise<void> {
  await new Promise((settle) => setTimeout(settle, 50));
}

describe('useReleasePipeline', () => {
  let client: QueryClient;

  beforeEach(() => {
    mockPipeline.mockReset();
    workspace = { id: 'workspace-1' };
    client = new QueryClient(QUIET);
  });

  it('leaves a settled release alone', async () => {
    const { result } = renderHook(() => useReleasePipeline(settled(pending(5))), {
      wrapper: provider(client),
    });
    await idle();
    expect(mockPipeline).not.toHaveBeenCalled();
    expect(result.current.watching).toBe(false);
  });

  it('leaves an active release alone once it is 30 minutes old', async () => {
    renderHook(() => useReleasePipeline(pending(31)), { wrapper: provider(client) });
    await idle();
    expect(mockPipeline).not.toHaveBeenCalled();
  });

  it('leaves the pipeline alone without a workspace', async () => {
    workspace = null;
    const { result } = renderHook(() => useReleasePipeline(pending(5)), {
      wrapper: provider(client),
    });
    await idle();
    expect(mockPipeline).not.toHaveBeenCalled();
    expect(result.current.watching).toBe(false);
  });

  it('waits out the poll interval when the list was checked moments ago', async () => {
    const { result } = renderHook(() => useReleasePipeline(pending(5, 0)), {
      wrapper: provider(client),
    });
    await idle();
    expect(result.current.watching).toBe(true);
    expect(mockPipeline).not.toHaveBeenCalled();
  });

  it('reads the pipeline of a young active release and patches every release list', async () => {
    const subject = pending(5);
    const other = release({ id: 41, tag: 'v1.2.2' });
    const done = settled(subject);
    client.setQueryData(keys.releaseWindowKey('workspace-1'), listing([subject, other]));
    client.setQueryData(keys.releaseWindowKey('workspace-1', 14), listing([subject]));
    mockPipeline.mockResolvedValue(verdict(done));

    renderHook(() => useReleasePipeline(subject), { wrapper: provider(client) });

    await waitFor(() => expect(mockPipeline).toHaveBeenCalledTimes(1));
    expect(mockPipeline.mock.calls[0].slice(0, 4)).toEqual(['workspace-1', SOURCE_ID, 42, {}]);
    for (const days of [undefined, 14]) {
      await waitFor(() =>
        expect(
          client.getQueryData<ReleasesResponse>(keys.releaseWindowKey('workspace-1', days))
            ?.releases[0].pipeline
        ).toEqual(done.pipeline)
      );
    }
    const patched = client.getQueryData<ReleasesResponse>(keys.releaseWindowKey('workspace-1'));
    expect(patched?.releases[0]).toMatchObject({ state: 'succeeded', succeeded: true });
    expect(patched?.releases[1]).toEqual(other);
  });

  it('polls every 15 seconds, only while visible, until the pipeline settles', async () => {
    const subject = pending(5);
    mockPipeline.mockResolvedValueOnce(verdict(subject));
    renderHook(() => useReleasePipeline(subject), { wrapper: provider(client) });
    await waitFor(() => expect(mockPipeline).toHaveBeenCalledTimes(1));

    expect(PIPELINE_POLL_MS).toBe(15_000);
    expect(interval(client, subject)).toBe(PIPELINE_POLL_MS);
    const query = client.getQueryCache().find({
      queryKey: keys.releasePipelineKey('workspace-1', SOURCE_ID, 42),
    });
    expect(query?.observers[0].options.refetchIntervalInBackground).toBe(false);

    client.setQueryData(
      keys.releasePipelineKey('workspace-1', SOURCE_ID, 42),
      verdict(settled(subject))
    );
    expect(interval(client, subject)).toBe(false);
  });

  it('backs off after a gateway failure', async () => {
    const subject = pending(5);
    mockPipeline.mockRejectedValue(new ApiError('Failed', HttpStatus.BadGateway));
    const { unmount } = renderHook(() => useReleasePipeline(subject), {
      wrapper: provider(client),
    });
    await waitFor(() => expect(mockPipeline).toHaveBeenCalledTimes(1));
    const query = client.getQueryCache().find({
      queryKey: keys.releasePipelineKey('workspace-1', SOURCE_ID, 42),
    });
    expect(query?.observers[0].options.retry).toBe(retryPolled);
    expect(query?.observers[0].options.retryDelay).toBe(retryDelay);
    await idle();
    expect(mockPipeline).toHaveBeenCalledTimes(1);
    unmount();
  });

  it('stops polling a release the server no longer lists', async () => {
    const subject = pending(5);
    mockPipeline.mockRejectedValue(new ApiError('No such release', HttpStatus.NotFound));
    const { result } = renderHook(() => useReleasePipeline(subject), {
      wrapper: provider(client),
    });
    await waitFor(() => expect(result.current.watching).toBe(false));
    expect(mockPipeline).toHaveBeenCalledTimes(1);
    expect(interval(client, subject)).toBe(false);

    const query = client.getQueryCache().find({
      queryKey: keys.releasePipelineKey('workspace-1', SOURCE_ID, 42),
    });
    const { refetchOnWindowFocus, refetchOnReconnect, refetchOnMount } =
      query?.observers[0].options ?? {};
    for (const refetch of [refetchOnWindowFocus, refetchOnReconnect, refetchOnMount]) {
      expect(typeof refetch === 'function' && query ? refetch(query) : refetch).toBe(false);
    }
    await idle();
    expect(mockPipeline).toHaveBeenCalledTimes(1);
  });

  it('keeps polling after any other failure', async () => {
    const subject = pending(5);
    mockPipeline.mockRejectedValue(new ApiError('Unreadable source', HttpStatus.Conflict));
    const { result } = renderHook(() => useReleasePipeline(subject), {
      wrapper: provider(client),
    });
    await waitFor(() =>
      expect(
        client.getQueryCache().find({
          queryKey: keys.releasePipelineKey('workspace-1', SOURCE_ID, 42),
        })?.state.error
      ).not.toBeNull()
    );
    expect(result.current.watching).toBe(true);
    expect(interval(client, subject)).toBe(PIPELINE_POLL_MS);
  });

  it('leaves the lists alone when the release is not in them', async () => {
    const subject = pending(5);
    const other = listing([release({ id: 41 })]);
    client.setQueryData(keys.releaseWindowKey('workspace-1'), other);
    mockPipeline.mockResolvedValue(verdict(settled(subject)));
    renderHook(() => useReleasePipeline(subject), { wrapper: provider(client) });
    await waitFor(() => expect(mockPipeline).toHaveBeenCalledTimes(1));
    await idle();
    expect(client.getQueryData(keys.releaseWindowKey('workspace-1'))).toBe(other);
  });
});
