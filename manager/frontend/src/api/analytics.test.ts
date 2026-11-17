import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import { wireOverview } from '../test/analytics';
import { ApiError } from './ApiError';
import { analyticsApi, LOAD_FAILED } from './analytics';
import { HttpStatus } from './HttpStatus';

const PERIOD_REFUSAL = 'period must be one of day, week, month, quarter';

async function rejection(promise: Promise<unknown>): Promise<unknown> {
  try {
    await promise;
  } catch (thrown) {
    return thrown;
  }
  throw new Error('Expected the request to be refused');
}

describe('analytics api', () => {
  let fetch: ReturnType<typeof spyOn>;

  beforeEach(() => {
    fetch = spyOn(globalThis, 'fetch');
    analyticsApi.setGetAccessToken(() => 'test-token');
  });

  afterEach(() => {
    fetch.mockRestore();
    analyticsApi.setGetAccessToken(() => null);
  });

  function requested(): { url: string; init: RequestInit } {
    const [url, init] = fetch.mock.calls[0] as [string, RequestInit];
    return { url, init };
  }

  it('reads the overview for a workspace and period with a bearer token', async () => {
    fetch.mockResolvedValueOnce(Response.json(wireOverview));

    const overview = await analyticsApi.overview('workspace/1', 'month');

    expect(overview.runs.outcomes.total).toBe(40);
    const { url, init } = requested();
    expect(url).toEndWith('/api/workspaces/workspace%2F1/analytics?period=month');
    expect(init.headers).toMatchObject({ Authorization: 'Bearer test-token' });
    expect(init.method).toBeUndefined();
  });

  it('passes the abort signal through so a superseded period stops loading', async () => {
    fetch.mockResolvedValueOnce(Response.json(wireOverview));
    const controller = new AbortController();

    await analyticsApi.overview('workspace-1', 'week', controller.signal);

    expect(requested().init.signal).toBe(controller.signal);
  });

  it('sends no authorization header without a session', async () => {
    analyticsApi.setGetAccessToken(() => null);
    fetch.mockResolvedValueOnce(Response.json(wireOverview));

    await analyticsApi.overview('workspace-1', 'week');

    expect(requested().init.headers).not.toHaveProperty('Authorization');
  });

  it('surfaces the error the server gives for a period it does not know', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ success: false, error: PERIOD_REFUSAL }, { status: HttpStatus.BadRequest })
    );

    const error = await rejection(analyticsApi.overview('workspace-1', 'week'));

    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(HttpStatus.BadRequest);
    expect((error as ApiError).message).toBe(`${LOAD_FAILED}: ${PERIOD_REFUSAL}`);
  });

  it('falls back to the status when a failure has no readable body', async () => {
    fetch.mockResolvedValueOnce(
      new Response('upstream down', { status: HttpStatus.InternalServerError })
    );

    const error = await rejection(analyticsApi.overview('workspace-1', 'week'));

    expect((error as ApiError).message).toBe(`${LOAD_FAILED}: 500`);
  });

  it('rejects a body that does not match the contract', async () => {
    fetch.mockResolvedValueOnce(Response.json({ ...wireOverview, truncated: 'no' }));

    const error = await rejection(analyticsApi.overview('workspace-1', 'week'));

    expect((error as Error).message).toContain('truncated');
  });
});
