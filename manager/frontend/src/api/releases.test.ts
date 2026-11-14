import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import { ReleaseRefusal } from '../features/releases/ReleaseRefusal';
import { ApiError } from './ApiError';
import { releasesApi } from './releases';

const identity = {
  repository: 'acme/zone',
  id: 42,
  tag: 'v1.2.3',
  published_at: '2026-11-12T09:00:00Z',
  url: 'https://github.com/acme/zone/releases/tag/v1.2.3',
};

const pipeline = {
  release: identity,
  lookup: 'pending',
  runs: [],
  checked_at: '2026-11-12T09:01:05Z',
};

const verdict = {
  state: 'unknown',
  succeeded: false,
  complete: false,
  blockers: [{ code: 'lookup_pending', detail: 'No run yet' }],
};

const listing = {
  generated_at: '2026-11-12T09:01:05Z',
  stale: false,
  partial: false,
  window_days: 14,
  ttl_secs: 120,
  sources: [],
  releases: [
    {
      ...verdict,
      source_id: 'source-1',
      repository: 'acme/zone',
      repository_url: 'https://github.com/acme/zone',
      id: 42,
      tag: 'v1.2.3',
      name: 'v1.2.3',
      url: identity.url,
      published_at: identity.published_at,
      prerelease: false,
      pulls: [],
      pipeline,
    },
  ],
  warnings: [],
};

const preview = {
  source_id: 'source/1',
  repository: 'acme/zone',
  tag: 'v1.2.3',
  name: 'v1.2.3',
  body: 'Notes',
  base_tag: 'v1.2.2',
  target: 'b'.repeat(40),
  prerelease: false,
  pulls: [],
  digest: 'c'.repeat(64),
};

const created = {
  source_id: 'source/1',
  id: 43,
  tag: 'v1.2.3',
  name: 'v1.2.3',
  url: 'https://github.com/acme/zone/releases/tag/v1.2.3',
  published_at: '2026-11-13T10:00:00Z',
};

async function rejection(promise: Promise<unknown>): Promise<ApiError> {
  try {
    await promise;
  } catch (thrown) {
    if (thrown instanceof ApiError) return thrown;
    throw thrown;
  }
  throw new Error('Expected the request to be refused');
}

async function refusal(promise: Promise<unknown>): Promise<ReleaseRefusal> {
  const error = await rejection(promise);
  if (error instanceof ReleaseRefusal) return error;
  throw new Error(`Expected a release refusal, got ${error.name}`);
}

const previewRequest = { source_id: 'source/1', tag: 'v1.2.3', expected_latest_tag: 'v1.2.2' };
const createRequest = { ...previewRequest, prerelease: false, digest: preview.digest };

describe('releases api', () => {
  let fetch: ReturnType<typeof spyOn>;
  beforeEach(() => {
    fetch = spyOn(globalThis, 'fetch');
    releasesApi.setGetAccessToken(() => 'test-token');
  });
  afterEach(() => {
    fetch.mockRestore();
    releasesApi.setGetAccessToken(() => null);
  });

  function requested(): { url: string; init: RequestInit } {
    const [url, init] = fetch.mock.calls[0] as [string, RequestInit];
    return { url, init };
  }

  it('lists releases with a bearer token and no query by default', async () => {
    fetch.mockResolvedValueOnce(Response.json(listing));
    const response = await releasesApi.list('workspace/1');
    expect(response.releases[0].tag).toBe('v1.2.3');
    const { url, init } = requested();
    expect(url).toEndWith('/api/workspaces/workspace%2F1/releases');
    expect(init.headers).toMatchObject({ Authorization: 'Bearer test-token' });
    expect(init.method).toBeUndefined();
  });

  it('asks for the window and a refresh when given', async () => {
    fetch.mockResolvedValueOnce(Response.json(listing));
    await releasesApi.list('workspace-1', { days: 14, refresh: true });
    expect(requested().url).toEndWith('/api/workspaces/workspace-1/releases?days=14&refresh=1');
  });

  it('leaves refresh out when it is not asked for', async () => {
    fetch.mockResolvedValueOnce(Response.json(listing));
    await releasesApi.list('workspace-1', { days: 7, refresh: false });
    expect(requested().url).toEndWith('/releases?days=7');
  });

  it('sends no bearer header without a token', async () => {
    releasesApi.setGetAccessToken(() => null);
    fetch.mockResolvedValueOnce(Response.json(listing));
    await releasesApi.list('workspace-1');
    expect(requested().init.headers).not.toHaveProperty('Authorization');
  });

  it('forwards the abort signal on reads', async () => {
    const controller = new AbortController();
    fetch.mockResolvedValueOnce(Response.json(listing));
    await releasesApi.list('workspace-1', {}, controller.signal);
    expect(requested().init.signal).toBe(controller.signal);
  });

  it('rejects a listing that does not match the contract', async () => {
    fetch.mockResolvedValueOnce(Response.json({ releases: 'none' }));
    await expect(releasesApi.list('workspace-1')).rejects.toThrow('Validation failed');
  });

  it('reads the options for every source', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ generated_at: '2026-11-12T09:00:00Z', sources: [], warnings: [] })
    );
    const response = await releasesApi.options('workspace/1', { refresh: true });
    expect(response.sources).toEqual([]);
    expect(requested().url).toEndWith('/api/workspaces/workspace%2F1/releases/options?refresh=1');
  });

  it('reads one release pipeline by source and release id', async () => {
    fetch.mockResolvedValueOnce(Response.json({ ...verdict, pipeline }));
    const response = await releasesApi.pipeline('workspace-1', 'source/1', 42);
    expect(response.pipeline.lookup).toBe('pending');
    expect(response.blockers[0].code).toBe('lookup_pending');
    expect(requested().url).toEndWith(
      '/api/workspaces/workspace-1/releases/source%2F1/42/pipeline'
    );
  });

  it('refreshes one release pipeline when asked', async () => {
    fetch.mockResolvedValueOnce(Response.json({ ...verdict, pipeline }));
    await releasesApi.pipeline('workspace-1', 'source-1', 42, { refresh: true });
    expect(requested().url).toEndWith('/releases/source-1/42/pipeline?refresh=1');
  });

  it('previews a release with the tag in the body', async () => {
    fetch.mockResolvedValueOnce(Response.json({ preview }));
    expect(await releasesApi.preview('workspace-1', previewRequest)).toEqual(preview);
    const { url, init } = requested();
    expect(url).toEndWith('/api/workspaces/workspace-1/releases/preview');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual(previewRequest);
  });

  it('creates a release with the reviewed digest', async () => {
    fetch.mockResolvedValueOnce(Response.json({ release: created }, { status: 201 }));
    expect(await releasesApi.create('workspace-1', createRequest)).toEqual(created);
    const { url, init } = requested();
    expect(url).toEndWith('/api/workspaces/workspace-1/releases');
    expect(init.method).toBe('POST');
    expect(init.headers).toMatchObject({ Authorization: 'Bearer test-token' });
    expect(JSON.parse(init.body as string)).toEqual(createRequest);
  });

  it('trims the tag it previews', async () => {
    fetch.mockResolvedValueOnce(Response.json({ preview }));
    await releasesApi.preview('workspace-1', {
      ...previewRequest,
      tag: `  ${previewRequest.tag} `,
    });
    expect(JSON.parse(requested().init.body as string).tag).toBe(previewRequest.tag);
  });

  it('refuses a blank tag or a missing digest without asking the server', async () => {
    await expect(
      releasesApi.preview('workspace-1', { ...previewRequest, tag: '  ' })
    ).rejects.toThrow('Tag is required');
    await expect(
      releasesApi.create('workspace-1', { ...createRequest, digest: '' })
    ).rejects.toThrow('digest');
    expect(fetch).not.toHaveBeenCalled();
  });

  it('throws a latest_tag_moved refusal from preview', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ success: false, error: 'Refused', code: 'latest_tag_moved' }, { status: 409 })
    );
    const error = await refusal(releasesApi.preview('workspace-1', previewRequest));
    expect(error).toBeInstanceOf(ApiError);
    expect(error.code).toBe('latest_tag_moved');
    expect(error.status).toBe(409);
    expect(error.message).toBe('Refused');
    expect(error.reconciliation).toBeNull();
    expect(error.tagRefusal).toBeNull();
  });

  it.each([
    { code: 'tag_exists', tag: 'v1.2.3' },
    { code: 'tag_not_newer', tag: 'v1.2.3', latest_tag: 'v1.3.0' },
    { code: 'release_exists', tag: 'v1.2.3', release_id: 91 },
  ])('throws a refusal of the tag from preview or create: $code', async (detail) => {
    for (const send of [
      () => releasesApi.preview('workspace-1', previewRequest),
      () => releasesApi.create('workspace-1', createRequest),
    ]) {
      fetch.mockResolvedValueOnce(
        Response.json({ success: false, error: 'Refused', ...detail }, { status: 409 })
      );
      const error = await refusal(send());
      expect(error.code).toBe(detail.code as typeof error.code);
      expect(error.tagRefusal).toMatchObject(detail);
      expect(error.reconciliation).toBeNull();
    }
  });

  it.each([
    [403, 'Missing permission: releases:create', 'Failed to create release'],
    [
      400,
      'prerelease must be true exactly when the tag is a prerelease',
      'Failed to create release',
    ],
    [500, 'The release stopped before it reported back', 'Failed to create release'],
  ])('names the action and the reason for a %i from create', async (status, reason, action) => {
    fetch.mockResolvedValueOnce(Response.json({ success: false, error: reason }, { status }));
    const error = await rejection(releasesApi.create('workspace-1', createRequest));
    expect(error).not.toBeInstanceOf(ReleaseRefusal);
    expect(error.status).toBe(status);
    expect(error.message).toBe(`${action}: ${reason}`);
  });

  it('names the reason for a forbidden preview sent without success', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ error: 'Missing permission: releases:create' }, { status: 403 })
    );
    const error = await rejection(releasesApi.preview('workspace-1', previewRequest));
    expect(error.status).toBe(403);
    expect(error.message).toBe('Failed to preview release: Missing permission: releases:create');
  });

  it.each(['preview_stale', 'release_in_progress'])(
    'throws a %s refusal from create',
    async (code) => {
      fetch.mockResolvedValueOnce(
        Response.json({ success: false, error: 'Refused', code }, { status: 409 })
      );
      const error = await refusal(releasesApi.create('workspace-1', createRequest));
      expect(error.code).toBe(code as typeof error.code);
    }
  );

  it('carries what is left to reconcile when a create stops halfway', async () => {
    const body = {
      success: false,
      error: 'The tag was written but the release was not published',
      code: 'release_manual_reconciliation_required',
      tag: 'v1.2.3',
      tag_object: 'e'.repeat(40),
      reference_created: null,
      release_id: 44,
      release_exists: false,
      marker: '0b9f1c52-6d4e-4a8b-9f2e-1c3d5e7f9a0b',
    };
    fetch.mockResolvedValueOnce(Response.json(body, { status: 409 }));
    const error = await refusal(releasesApi.create('workspace-1', createRequest));
    expect(error.code).toBe('release_manual_reconciliation_required');
    expect(error.tagRefusal).toBeNull();
    expect(error.reconciliation).toMatchObject({
      tag: 'v1.2.3',
      tag_object: 'e'.repeat(40),
      reference_created: null,
      release_id: 44,
      release_exists: false,
      marker: body.marker,
    });
  });

  it('throws a plain api error for a conflict it cannot read', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ success: false, error: 'Conflict', code: 'head_moved' }, { status: 409 })
    );
    const error = await rejection(releasesApi.create('workspace-1', createRequest));
    expect(error).not.toBeInstanceOf(ReleaseRefusal);
    expect(error.status).toBe(409);
    expect(error.message).toBe('Failed to create release: Conflict');
  });

  it('names the action and the server reason for an invalid tag', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ success: false, error: 'Invalid tag' }, { status: 400 })
    );
    const error = await rejection(releasesApi.preview('workspace-1', previewRequest));
    expect(error.status).toBe(400);
    expect(error.message).toBe('Failed to preview release: Invalid tag');
  });

  it('reads the reason from a message when the server sends no error', async () => {
    fetch.mockResolvedValueOnce(Response.json({ message: 'Source is gone' }, { status: 404 }));
    const error = await rejection(releasesApi.options('workspace-1'));
    expect(error.status).toBe(404);
    expect(error.message).toBe('Failed to fetch release options: Source is gone');
  });

  it('falls back to the status when a pipeline failure carries no reason', async () => {
    fetch.mockResolvedValueOnce(Response.json({}, { status: 500 }));
    const error = await rejection(releasesApi.pipeline('workspace-1', 'source-1', 42));
    expect(error.message).toBe('Failed to fetch release pipeline: 500');
  });

  it('keeps the status of a gateway failure so callers can back off', async () => {
    fetch.mockResolvedValueOnce(new Response('upstream down', { status: 502 }));
    const error = await rejection(releasesApi.list('workspace-1'));
    expect(error.status).toBe(502);
    expect(error.message).toBe('Failed to fetch releases: 502');
  });
});
