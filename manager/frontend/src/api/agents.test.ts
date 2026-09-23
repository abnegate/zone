import { afterEach, describe, expect, it, mock } from 'bun:test';
import fixture from '../../../../runner/zone_server/tests/fixtures/agents.json';
import {
  AgentLoginSchema,
  AgentStatusesSchema,
  AgentStatusSchema,
} from '../features/settings/ai/schemas';
import { AgentRequestError } from './AgentRequestError';
import { agentsApi } from './agents';
import { client } from './client';

const original = globalThis.fetch;
const organization = '00000000-0000-0000-0000-000000000001';
const agents = `/api/organizations/${organization}/agents`;
const authorize =
  'https://claude.com/cai/oauth/authorize?code=true&client_id=fake-client&response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&scope=user%3Ainference&code_challenge=fake-challenge&code_challenge_method=S256&state=fake-state';
const refusal =
  'Error logging in with device code: device code request failed with status 403 Forbidden';

function respond(body: unknown, status = 200) {
  const request = mock(async (_url: string, _init?: RequestInit) =>
    body === null ? new Response(null, { status }) : Response.json(body, { status })
  );
  globalThis.fetch = request as unknown as typeof fetch;
  return request;
}

function sent(request: ReturnType<typeof respond>) {
  const [url, init] = request.mock.calls[0];
  return { url, method: init?.method ?? 'GET', body: init?.body };
}

afterEach(() => {
  globalThis.fetch = original;
  client.setAccessToken(null);
});

describe('agent status contract', () => {
  it('parses the fixture the server serialises its statuses to', () => {
    const { agents: statuses } = AgentStatusesSchema.parse(fixture);
    expect(statuses.map((status) => [status.agent, status.provider, status.state])).toEqual([
      ['claude', 'claude_code', 'signed_in'],
      ['codex', 'codex', 'pending'],
    ]);
    expect(statuses[0].models).toEqual(['sonnet', 'opus', 'haiku']);
    expect(statuses[1].models).toEqual(['gpt-6-astra', 'gpt-6-sol', 'gpt-6-luna']);
    expect(statuses[1].pending?.user_code).toBe('ABCD-EFGHI');
  });

  it('accepts a pending sign-in whose code the server hides from a member', () => {
    const codex = fixture.agents[1];
    const hidden = { ...codex, pending: { ...codex.pending, user_code: null } };
    expect(AgentStatusSchema.parse(hidden).pending?.user_code).toBeNull();
  });

  it('refuses a sign-in link that is not a web address', () => {
    const login = { agent: 'claude', authorize_url: authorize, expires_at: '2026-09-23T04:10:00Z' };
    expect(AgentLoginSchema.safeParse(login).success).toBe(true);
    expect(
      AgentLoginSchema.safeParse({ ...login, authorize_url: 'javascript:alert(1)' }).success
    ).toBe(false);
    const device = {
      agent: 'codex',
      verification_url: 'data:text/html,<script>alert(1)</script>',
      user_code: 'ABCD-EFGHI',
      expires_at: '2026-09-23T04:15:00Z',
    };
    expect(AgentLoginSchema.safeParse(device).success).toBe(false);
  });
});

describe('agentsApi', () => {
  it('lists both agents of the organization', async () => {
    const request = respond(fixture);
    const statuses = await agentsApi.list(organization);
    expect(statuses.map((status) => status.state)).toEqual(['signed_in', 'pending']);
    expect(sent(request)).toEqual({ url: agents, method: 'GET', body: undefined });
  });

  it('reads one agent', async () => {
    const request = respond(fixture.agents[1]);
    const status = await agentsApi.get(organization, 'codex');
    expect(status.pending?.verification_url).toBe('https://auth.openai.com/codex/device');
    expect(sent(request).url).toBe(`${agents}/codex`);
  });

  it('starts a claude sign-in at the inference scope by default', async () => {
    const request = respond({
      agent: 'claude',
      authorize_url: authorize,
      expires_at: '2026-09-23T04:10:00Z',
    });
    const login = await agentsApi.start(organization, 'claude');
    expect(login).toEqual({
      agent: 'claude',
      authorize_url: authorize,
      expires_at: '2026-09-23T04:10:00Z',
    });
    expect(sent(request)).toEqual({ url: `${agents}/claude/login`, method: 'POST', body: '{}' });
  });

  it('asks claude for full access when told to', async () => {
    const request = respond({
      agent: 'claude',
      authorize_url: authorize,
      expires_at: '2026-09-23T04:10:00Z',
    });
    await agentsApi.start(organization, 'claude', 'full');
    expect(sent(request).body).toBe('{"scope":"full"}');
  });

  it('starts a codex device login and returns its code', async () => {
    const prompt = {
      agent: 'codex',
      verification_url: 'https://auth.openai.com/codex/device',
      user_code: 'ABCD-EFGHI',
      expires_at: '2026-09-23T04:15:00Z',
    };
    const request = respond(prompt);
    expect(await agentsApi.start(organization, 'codex')).toEqual(prompt);
    expect(sent(request)).toEqual({ url: `${agents}/codex/login`, method: 'POST', body: '{}' });
  });

  it('submits a pasted callback address as the claude code', async () => {
    const request = respond(fixture.agents[0]);
    const callback =
      'https://platform.claude.com/oauth/code/callback?code=fake-code&state=fake-state';
    const status = await agentsApi.submitCode(organization, callback);
    expect(status.state).toBe('signed_in');
    expect(sent(request)).toEqual({
      url: `${agents}/claude/login/code`,
      method: 'POST',
      body: JSON.stringify({ code: callback }),
    });
  });

  it('signs out', async () => {
    const request = respond(null, 204);
    await agentsApi.signOut(organization, 'codex');
    expect(sent(request)).toEqual({
      url: `${agents}/codex/login`,
      method: 'DELETE',
      body: undefined,
    });
  });

  it("surfaces the server's own reason when a sign-in fails", async () => {
    respond({ error: refusal }, 502);
    await expect(agentsApi.start(organization, 'codex')).rejects.toThrow(refusal);
    respond({ error: 'Only organization admins can sign in to coding agents' }, 403);
    await expect(agentsApi.start(organization, 'claude')).rejects.toThrow(
      'Only organization admins can sign in to coding agents'
    );
  });

  it('says whether a failed code can be pasted again or the sign-in has to start over', async () => {
    respond({ error: 'The code could not be read.', kind: 'invalid_code' }, 400);
    const unreadable = await agentsApi
      .submitCode(organization, 'nonsense')
      .catch((reason) => reason);
    expect(unreadable).toBeInstanceOf(AgentRequestError);
    expect(unreadable).toMatchObject({
      message: 'The code could not be read.',
      status: 400,
      kind: 'invalid_code',
    });

    respond({ error: 'Claude rejected the code. Start again.', kind: 'start_again' }, 502);
    const spent = await agentsApi.submitCode(organization, 'code#state').catch((reason) => reason);
    expect(spent).toMatchObject({ status: 502, kind: 'start_again' });
  });

  it('keeps the reason of a failure whose kind it does not know, without a kind', async () => {
    respond({ error: 'Something new went wrong.', kind: 'surprise' }, 400);
    const failure = await agentsApi
      .submitCode(organization, 'code#state')
      .catch((reason) => reason);
    expect(failure).toMatchObject({ message: 'Something new went wrong.', status: 400 });
    expect(failure.kind).toBeUndefined();

    respond({ error: 'Only organization admins can sign in to coding agents' }, 403);
    const denied = await agentsApi.start(organization, 'claude').catch((reason) => reason);
    expect(denied).toMatchObject({ status: 403 });
    expect(denied.kind).toBeUndefined();
  });

  it('names the status code when the failure has no readable reason', async () => {
    globalThis.fetch = mock(
      async () => new Response('<html>Bad Gateway</html>', { status: 502 })
    ) as unknown as typeof fetch;
    await expect(agentsApi.list(organization)).rejects.toThrow('502');
  });

  it("sends the signed-in user's token with every request", async () => {
    client.setAccessToken('fake-access-token');
    const request = respond(fixture);
    await agentsApi.list(organization);
    const [, init] = request.mock.calls[0];
    expect(new Headers(init?.headers).get('Authorization')).toBe('Bearer fake-access-token');
    expect(new Headers(init?.headers).get('Content-Type')).toBe('application/json');
  });

  it('sends no token once signed out', async () => {
    const request = respond(fixture);
    await agentsApi.list(organization);
    const [, init] = request.mock.calls[0];
    expect(new Headers(init?.headers).has('Authorization')).toBe(false);
  });

  it('encodes the organization in every path', async () => {
    const request = respond(fixture);
    await agentsApi.list('org/../1');
    expect(sent(request).url).toBe('/api/organizations/org%2F..%2F1/agents');
  });
});
