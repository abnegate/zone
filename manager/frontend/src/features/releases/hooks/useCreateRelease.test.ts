import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { notifyManager, QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { HEAD, SOURCE_ID as PULL_SOURCE } from '../../../test/pullRequests';
import { preview, SOURCE_ID } from '../../../test/releases';
import { NO_WORKSPACE } from '../../github';
import { pullRequestKeys } from '../../pull-requests';
import { ReleaseRefusal } from '../ReleaseRefusal';
import type { Created } from '../types';

const mockPreview = mock();
const mockCreate = mock();
const mockList = mock();

mock.module('../../../api/releases', () => ({
  releasesApi: { preview: mockPreview, create: mockCreate, list: mockList },
}));

let workspace: { id: string } | null = { id: 'workspace-1' };

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: workspace, loading: false }),
}));

let useCreateRelease: typeof import('./useCreateRelease').useCreateRelease;
let useReleases: typeof import('./useReleases').useReleases;
let keys: typeof import('./keys');

beforeAll(async () => {
  notifyManager.setScheduler(queueMicrotask);
  ({ useCreateRelease } = await import('./useCreateRelease'));
  ({ useReleases } = await import('./useReleases'));
  keys = await import('./keys');
});

afterAll(() => {
  notifyManager.setScheduler((callback) => setTimeout(callback, 0));
  mock.restore();
});

const QUIET = {
  defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
};

const SETTLE_MS = 1_000;

function provider(client: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

function within<T>(promise: Promise<T>, milliseconds: number): Promise<T> {
  return Promise.race([
    promise,
    new Promise<T>((_, reject) =>
      setTimeout(() => reject(new Error(`Still pending after ${milliseconds}ms`)), milliseconds)
    ),
  ]);
}

const previewRequest = { source_id: SOURCE_ID, tag: 'v1.2.4', expected_latest_tag: 'v1.2.3' };
const createRequest = { ...previewRequest, prerelease: false, digest: preview.digest };

const created: Created = {
  source_id: SOURCE_ID,
  id: 43,
  tag: 'v1.2.4',
  name: 'v1.2.4',
  url: 'https://github.com/acme/zone/releases/tag/v1.2.4',
  published_at: '2026-11-13T10:00:00Z',
};

const revision = { source_id: PULL_SOURCE, number: 42, head: HEAD };

function seeded(): QueryClient {
  const client = new QueryClient(QUIET);
  for (const key of [
    keys.releaseWindowKey('workspace-1'),
    keys.releaseOptionsKey('workspace-1'),
    keys.releasePipelineKey('workspace-1', SOURCE_ID, 42),
    pullRequestKeys.list('workspace-1'),
    pullRequestKeys.detail('workspace-1', revision),
    pullRequestKeys.diff('workspace-1', revision),
    pullRequestKeys.commits('workspace-1', revision),
    pullRequestKeys.commit('workspace-1', revision, HEAD),
    pullRequestKeys.log('workspace-1', revision, 9001),
    keys.releaseWindowKey('workspace-2'),
    pullRequestKeys.list('workspace-2'),
  ]) {
    client.setQueryData(key, {});
  }
  return client;
}

function invalidated(client: QueryClient, queryKey: readonly unknown[]): boolean | undefined {
  return client.getQueryCache().find({ queryKey, exact: true })?.state.isInvalidated;
}

function expectListingsInvalidated(client: QueryClient) {
  expect(invalidated(client, keys.releaseWindowKey('workspace-1'))).toBe(true);
  expect(invalidated(client, keys.releaseOptionsKey('workspace-1'))).toBe(true);
  expect(invalidated(client, keys.releasePipelineKey('workspace-1', SOURCE_ID, 42))).toBe(true);
  expect(invalidated(client, pullRequestKeys.list('workspace-1'))).toBe(true);
  expect(invalidated(client, pullRequestKeys.detail('workspace-1', revision))).toBe(true);
}

function expectArtifactsKept(client: QueryClient) {
  expect(invalidated(client, pullRequestKeys.diff('workspace-1', revision))).toBe(false);
  expect(invalidated(client, pullRequestKeys.commits('workspace-1', revision))).toBe(false);
  expect(invalidated(client, pullRequestKeys.commit('workspace-1', revision, HEAD))).toBe(false);
  expect(invalidated(client, pullRequestKeys.log('workspace-1', revision, 9001))).toBe(false);
  expect(invalidated(client, keys.releaseWindowKey('workspace-2'))).toBe(false);
  expect(invalidated(client, pullRequestKeys.list('workspace-2'))).toBe(false);
}

function expectNothingInvalidated(client: QueryClient) {
  expect(
    client
      .getQueryCache()
      .getAll()
      .filter((query) => query.state.isInvalidated)
  ).toEqual([]);
}

describe('useCreateRelease', () => {
  let client: QueryClient;

  beforeEach(() => {
    mockPreview.mockReset();
    mockCreate.mockReset();
    mockList.mockReset();
    workspace = { id: 'workspace-1' };
    client = seeded();
  });

  it('previews a release in the workspace without invalidating anything', async () => {
    mockPreview.mockResolvedValue(preview);
    const { result } = renderHook(() => useCreateRelease(), { wrapper: provider(client) });

    let previewed: unknown;
    await act(async () => {
      previewed = await result.current.preview(previewRequest);
    });

    expect(previewed).toEqual(preview);
    expect(mockPreview).toHaveBeenCalledWith('workspace-1', previewRequest);
    expectNothingInvalidated(client);
  });

  it('creates a release and invalidates the releases and the pull request listings', async () => {
    mockCreate.mockResolvedValue(created);
    const { result } = renderHook(() => useCreateRelease(), { wrapper: provider(client) });

    let release: unknown;
    await act(async () => {
      release = await result.current.create(createRequest);
    });

    expect(release).toEqual(created);
    expect(mockCreate).toHaveBeenCalledWith('workspace-1', createRequest);
    expectListingsInvalidated(client);
    expectArtifactsKept(client);
  });

  it('resolves a create while the mounted release list keeps answering 502', async () => {
    mockList.mockRejectedValue(new ApiError('Bad gateway', HttpStatus.BadGateway));
    mockCreate.mockResolvedValue(created);
    const { result } = renderHook(
      () => {
        const releases = useReleases();
        return { releases, creation: useCreateRelease() };
      },
      { wrapper: provider(new QueryClient(QUIET)) }
    );
    await waitFor(() => expect(result.current.releases.unavailable).toBe(true));

    let release: unknown;
    await act(async () => {
      release = await within(result.current.creation.create(createRequest), SETTLE_MS);
    });

    expect(release).toEqual(created);
  });

  it('invalidates when a create stops after writing the tag', async () => {
    const refusal = new ReleaseRefusal({
      error: 'The tag was written but the release was not published',
      code: 'release_manual_reconciliation_required',
      tag: 'v1.2.4',
      tag_object: 'e'.repeat(40),
      reference_created: true,
      release_id: null,
      release_exists: false,
      marker: '0b9f1c52-6d4e-4a8b-9f2e-1c3d5e7f9a0b',
    });
    mockCreate.mockRejectedValue(refusal);
    const { result } = renderHook(() => useCreateRelease(), { wrapper: provider(client) });

    await act(() => expect(result.current.create(createRequest)).rejects.toBe(refusal));

    expectListingsInvalidated(client);
    expectArtifactsKept(client);
  });

  it('invalidates when a create stopped before saying what it wrote', async () => {
    const unknown = new ApiError(
      'Failed to create release: The release stopped before it reported back',
      HttpStatus.InternalServerError
    );
    mockCreate.mockRejectedValue(unknown);
    const { result } = renderHook(() => useCreateRelease(), { wrapper: provider(client) });

    await act(() => expect(result.current.create(createRequest)).rejects.toBe(unknown));

    expectListingsInvalidated(client);
    expectArtifactsKept(client);
  });

  it('invalidates nothing when a create is forbidden', async () => {
    const forbidden = new ApiError(
      'Failed to create release: Missing permission: releases:create',
      HttpStatus.Forbidden
    );
    mockCreate.mockRejectedValue(forbidden);
    const { result } = renderHook(() => useCreateRelease(), { wrapper: provider(client) });

    await act(() => expect(result.current.create(createRequest)).rejects.toBe(forbidden));

    expectNothingInvalidated(client);
  });

  it('invalidates nothing when a create is refused before writing', async () => {
    const refusal = new ReleaseRefusal({ error: 'Preview is stale', code: 'preview_stale' });
    mockCreate.mockRejectedValue(refusal);
    const { result } = renderHook(() => useCreateRelease(), { wrapper: provider(client) });

    await act(() => expect(result.current.create(createRequest)).rejects.toBe(refusal));

    expect(result.current.creating).toBe(false);
    expectNothingInvalidated(client);
  });

  it('hands a refused preview back so the wizard can offer to reload options', async () => {
    const refusal = new ReleaseRefusal({
      error: 'v1.2.4 exists',
      code: 'tag_exists',
      tag: 'v1.2.4',
    });
    mockPreview.mockRejectedValue(refusal);
    const { result } = renderHook(() => useCreateRelease(), { wrapper: provider(client) });

    await act(() => expect(result.current.preview(previewRequest)).rejects.toBe(refusal));

    expect(result.current.previewing).toBe(false);
    expectNothingInvalidated(client);
  });

  it('reports both requests as pending while they run', async () => {
    let finish: (value: Created) => void = () => {};
    mockCreate.mockReturnValue(
      new Promise<Created>((resolve) => {
        finish = resolve;
      })
    );
    const { result } = renderHook(() => useCreateRelease(), { wrapper: provider(client) });

    let pending: Promise<Created> = Promise.resolve(created);
    act(() => {
      pending = result.current.create(createRequest);
    });
    await waitFor(() => expect(result.current.creating).toBe(true));
    expect(result.current.previewing).toBe(false);

    await act(async () => {
      finish(created);
      await expect(pending).resolves.toEqual(created);
    });
    expect(result.current.creating).toBe(false);
  });

  it('refuses to preview or create without a workspace', async () => {
    workspace = null;
    const { result } = renderHook(() => useCreateRelease(), { wrapper: provider(client) });
    await expect(result.current.preview(previewRequest)).rejects.toThrow(NO_WORKSPACE);
    await expect(result.current.create(createRequest)).rejects.toThrow(NO_WORKSPACE);
    expect(mockPreview).not.toHaveBeenCalled();
    expect(mockCreate).not.toHaveBeenCalled();
  });
});
