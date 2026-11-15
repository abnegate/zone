import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import { PullRequestRefusal } from '../features/pull-requests/PullRequestRefusal';
import {
  HEAD,
  wireCommits,
  wireDetail,
  wireDiff,
  wireLog,
  wireMerged,
  wirePullRequests,
} from '../test/pullRequests';
import { ApiError } from './ApiError';
import { pullRequestsApi } from './pullRequests';

const WORKSPACE = 'workspace/1';
const REFERENCE = { source_id: 'source/1', number: 42 };
const ENCODED = '/api/workspaces/workspace%2F1/pull-requests/source%2F1/42';

function requested(fetch: ReturnType<typeof spyOn>): { url: string; init: RequestInit } {
  const [url, init] = fetch.mock.calls[0] as [string, RequestInit];
  return { url, init };
}

describe('pull requests api', () => {
  let fetch: ReturnType<typeof spyOn>;

  beforeEach(() => {
    fetch = spyOn(globalThis, 'fetch');
    pullRequestsApi.setGetAccessToken(() => 'test-token');
  });

  afterEach(() => {
    fetch.mockRestore();
    pullRequestsApi.setGetAccessToken(() => null);
  });

  describe('list', () => {
    it('reads the workspace listing with the bearer token', async () => {
      fetch.mockResolvedValueOnce(Response.json(wirePullRequests));
      const listing = await pullRequestsApi.list(WORKSPACE);
      expect(listing.pull_requests[0].number).toBe(42);
      const { url, init } = requested(fetch);
      expect(url).toEndWith('/api/workspaces/workspace%2F1/pull-requests');
      expect(init.method).toBeUndefined();
      expect(init.headers).toMatchObject({ Authorization: 'Bearer test-token' });
    });

    it('asks the server to refresh only when told to', async () => {
      fetch.mockResolvedValueOnce(Response.json(wirePullRequests));
      await pullRequestsApi.list(WORKSPACE, true);
      expect(requested(fetch).url).toEndWith('/pull-requests?refresh=1');
    });

    it('sends no bearer header without a token', async () => {
      pullRequestsApi.setGetAccessToken(() => null);
      fetch.mockResolvedValueOnce(Response.json(wirePullRequests));
      await pullRequestsApi.list(WORKSPACE);
      expect(requested(fetch).init.headers).not.toHaveProperty('Authorization');
    });

    it('forwards the abort signal', async () => {
      const controller = new AbortController();
      fetch.mockResolvedValueOnce(Response.json(wirePullRequests));
      await pullRequestsApi.list(WORKSPACE, false, controller.signal);
      expect(requested(fetch).init.signal).toBe(controller.signal);
    });

    it('reports a bad gateway with its status and the server error', async () => {
      fetch.mockResolvedValueOnce(
        Response.json({ success: false, error: 'GitHub is unavailable' }, { status: 502 })
      );
      const failure = await pullRequestsApi.list(WORKSPACE).catch((error: unknown) => error);
      expect(failure).toBeInstanceOf(ApiError);
      expect(failure).not.toBeInstanceOf(PullRequestRefusal);
      expect((failure as ApiError).status).toBe(502);
      expect((failure as ApiError).message).toBe(
        'Failed to fetch pull requests: GitHub is unavailable'
      );
    });

    it('rejects a listing that does not match the contract', async () => {
      fetch.mockResolvedValueOnce(Response.json({ pull_requests: 'none' }));
      await expect(pullRequestsApi.list(WORKSPACE)).rejects.toThrow('Validation failed');
    });
  });

  it('reads one pull request with its threads and bot comments', async () => {
    fetch.mockResolvedValueOnce(Response.json(wireDetail));
    const detail = await pullRequestsApi.detail(WORKSPACE, REFERENCE);
    expect(detail.threads).toHaveLength(1);
    expect(detail.comments).toHaveLength(1);
    expect(requested(fetch).url).toEndWith(ENCODED);
  });

  it('reads the diff at the head it was shown', async () => {
    fetch.mockResolvedValueOnce(Response.json({ diff: wireDiff }));
    const diff = await pullRequestsApi.diff(WORKSPACE, REFERENCE, HEAD);
    expect(diff.files[0].path).toBe('src/main.rs');
    expect(requested(fetch).url).toEndWith(`${ENCODED}/diff?head=${HEAD}`);
  });

  it('reads the commits at the head it was shown', async () => {
    fetch.mockResolvedValueOnce(Response.json({ commits: wireCommits }));
    const commits = await pullRequestsApi.commits(WORKSPACE, REFERENCE, HEAD);
    expect(commits.commits).toHaveLength(1);
    expect(requested(fetch).url).toEndWith(`${ENCODED}/commits?head=${HEAD}`);
  });

  it('reads one commit diff with the sha encoded', async () => {
    fetch.mockResolvedValueOnce(Response.json({ diff: wireDiff }));
    await pullRequestsApi.commitDiff(WORKSPACE, REFERENCE, 'c/1', HEAD);
    expect(requested(fetch).url).toEndWith(`${ENCODED}/commits/c%2F1?head=${HEAD}`);
  });

  it('reads a check log by job id', async () => {
    fetch.mockResolvedValueOnce(Response.json({ log: wireLog }));
    const log = await pullRequestsApi.jobLog(WORKSPACE, REFERENCE, 9001, HEAD);
    expect(log.text).toBe('error: test failed');
    expect(requested(fetch).url).toEndWith(`${ENCODED}/jobs/9001/log?head=${HEAD}`);
  });

  it('encodes a head that is not a bare sha', async () => {
    fetch.mockResolvedValueOnce(Response.json({ diff: wireDiff }));
    await pullRequestsApi.diff(WORKSPACE, REFERENCE, 'a&b=c');
    expect(requested(fetch).url).toEndWith('/diff?head=a%26b%3Dc');
  });

  describe('merge', () => {
    it('posts the expected head and method and reads the receipt', async () => {
      fetch.mockResolvedValueOnce(Response.json({ merged: wireMerged }));
      const merged = await pullRequestsApi.merge(WORKSPACE, REFERENCE, {
        expected_head: HEAD,
        method: 'squash',
      });
      expect(merged).toEqual(wireMerged);
      const { url, init } = requested(fetch);
      expect(url).toEndWith(`${ENCODED}/merge`);
      expect(init.method).toBe('POST');
      expect(init.headers).toMatchObject({
        Authorization: 'Bearer test-token',
        'Content-Type': 'application/json',
      });
      expect(JSON.parse(init.body as string)).toEqual({ expected_head: HEAD, method: 'squash' });
    });

    it('leaves the method out when none was chosen', async () => {
      fetch.mockResolvedValueOnce(Response.json({ merged: wireMerged }));
      await pullRequestsApi.merge(WORKSPACE, REFERENCE, { expected_head: HEAD });
      expect(JSON.parse(requested(fetch).init.body as string)).toEqual({ expected_head: HEAD });
    });

    it('refuses to send a merge without an expected head', async () => {
      await expect(
        pullRequestsApi.merge(WORKSPACE, REFERENCE, { expected_head: '' })
      ).rejects.toThrow('Validation failed');
      expect(fetch).not.toHaveBeenCalled();
    });

    it('throws a moved head as a refusal carrying the new head', async () => {
      const current = 'd'.repeat(40);
      fetch.mockResolvedValueOnce(
        Response.json(
          {
            success: false,
            error: 'The pull request head moved',
            code: 'head_moved',
            current_head: current,
          },
          { status: 409 }
        )
      );
      const failure = await pullRequestsApi
        .merge(WORKSPACE, REFERENCE, { expected_head: HEAD })
        .catch((error: unknown) => error);
      expect(failure).toBeInstanceOf(PullRequestRefusal);
      expect(failure).toBeInstanceOf(ApiError);
      const refusal = failure as PullRequestRefusal;
      expect(refusal.name).toBe('PullRequestRefusal');
      expect(refusal.status).toBe(409);
      expect(refusal.code).toBe('head_moved');
      expect(refusal.currentHead).toBe(current);
      expect(refusal.blockers).toEqual([]);
      expect(refusal.message).toBe('The pull request head moved');
    });

    it('throws a not-ready refusal carrying its blockers', async () => {
      const blockers = [{ code: 'checks_failed', detail: '1 check failed' }];
      fetch.mockResolvedValueOnce(
        Response.json(
          { success: false, error: 'Not ready', code: 'not_ready', blockers },
          { status: 409 }
        )
      );
      const failure = await pullRequestsApi
        .merge(WORKSPACE, REFERENCE, { expected_head: HEAD })
        .catch((error: unknown) => error);
      expect((failure as PullRequestRefusal).code).toBe('not_ready');
      expect((failure as PullRequestRefusal).currentHead).toBeNull();
      expect((failure as PullRequestRefusal).blockers).toEqual(blockers);
    });

    const codes = ['conflict', 'protected', 'merged', 'closed'];

    it.each(codes)('throws a %s refusal with its code', async (code) => {
      fetch.mockResolvedValueOnce(
        Response.json({ success: false, error: 'Refused', code }, { status: 409 })
      );
      const failure = await pullRequestsApi
        .merge(WORKSPACE, REFERENCE, { expected_head: HEAD })
        .catch((error: unknown) => error);
      expect((failure as PullRequestRefusal).code).toBe(code as never);
    });

    it('reports a conflict without a refusal code as a plain failure', async () => {
      fetch.mockResolvedValueOnce(
        Response.json({ success: false, error: 'Busy' }, { status: 409 })
      );
      const failure = await pullRequestsApi
        .merge(WORKSPACE, REFERENCE, { expected_head: HEAD })
        .catch((error: unknown) => error);
      expect(failure).not.toBeInstanceOf(PullRequestRefusal);
      expect((failure as ApiError).status).toBe(409);
      expect((failure as ApiError).message).toBe('Failed to merge pull request: Busy');
    });

    it.each([
      [403, 'Missing permission: pull_requests:update'],
      [400, 'Failed to deserialize the JSON body into the target type'],
      [500, 'The merge stopped before it reported back, so whether it happened is unknown.'],
      [502, 'GitHub refused the credential.'],
    ])('names the reason for a %i from a merge', async (status, reason) => {
      fetch.mockResolvedValueOnce(Response.json({ error: reason }, { status }));
      const failure = await pullRequestsApi
        .merge(WORKSPACE, REFERENCE, { expected_head: HEAD })
        .catch((error: unknown) => error);
      expect(failure).not.toBeInstanceOf(PullRequestRefusal);
      expect((failure as ApiError).status).toBe(status);
      expect((failure as ApiError).message).toBe(`Failed to merge pull request: ${reason}`);
    });

    it('reports a conflict with an unreadable body as a plain failure', async () => {
      fetch.mockResolvedValueOnce(new Response('<html>', { status: 409 }));
      const failure = await pullRequestsApi
        .merge(WORKSPACE, REFERENCE, { expected_head: HEAD })
        .catch((error: unknown) => error);
      expect(failure).not.toBeInstanceOf(PullRequestRefusal);
      expect((failure as ApiError).message).toBe('Failed to merge pull request: 409');
    });
  });

  it('throws a moved head on a head-keyed read as a refusal', async () => {
    fetch.mockResolvedValueOnce(
      Response.json(
        { success: false, error: 'Head moved', code: 'head_moved', current_head: 'e'.repeat(40) },
        { status: 409 }
      )
    );
    const failure = await pullRequestsApi
      .diff(WORKSPACE, REFERENCE, HEAD)
      .catch((error: unknown) => error);
    expect((failure as PullRequestRefusal).currentHead).toBe('e'.repeat(40));
  });
});
