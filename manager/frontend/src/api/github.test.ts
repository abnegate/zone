import { afterEach, describe, expect, it, mock } from 'bun:test';
import { ApiError } from './ApiError';
import { githubApi } from './github';
import { session } from './session';

const original = globalThis.fetch;
const base = '/api/organizations/org-1/github';

const app = {
  id: 'app-1',
  app_id: 101,
  slug: 'zone-acme',
  name: 'Zone (acme)',
  owner_login: 'acme',
  owner_type: 'Organization',
  html_url: 'https://github.com/apps/zone-acme',
  settings_url: 'https://github.com/organizations/acme/settings/apps/zone-acme',
  created_at: '2026-11-01T00:00:00Z',
};

const installation = {
  id: 'inst-1',
  github_app_id: 'app-1',
  installation_id: 5001,
  account_login: 'acme',
  account_type: 'Organization',
  repository_selection: 'all',
  html_url: 'https://github.com/organizations/acme/settings/installations/5001',
  suspended_at: null,
  revoked_at: null,
  created_at: '2026-11-01T00:05:00Z',
};

const repository = {
  installation_id: 'inst-1',
  id: 9001,
  owner: 'acme',
  name: 'zone',
  full_name: 'acme/zone',
  private: true,
  archived: false,
  default_branch: 'main',
  html_url: 'https://github.com/acme/zone',
};

const repositories = {
  repositories: [repository],
  installations: [
    {
      id: 'inst-1',
      account_login: 'acme',
      html_url: installation.html_url,
      error: null,
    },
  ],
  total: 1,
  next_page: null,
};

const branches = {
  branches: [
    { name: 'main', protected: true },
    { name: 'feature/picker', protected: false },
  ],
  next_page: 2,
};

function respond(body: unknown, status = 200) {
  const request = mock(async (_url: string, _init?: RequestInit) =>
    body === null ? new Response(null, { status }) : Response.json(body, { status })
  );
  globalThis.fetch = request as unknown as typeof fetch;
  return request;
}

function sent(request: ReturnType<typeof respond>) {
  const [url, init] = request.mock.calls[0];
  return { url, method: init?.method ?? 'GET', body: init?.body ?? undefined };
}

afterEach(() => {
  globalThis.fetch = original;
  session.setAccessToken(null);
});

describe('githubApi.getConnection', () => {
  it('reads the connected apps and their installations', async () => {
    const request = respond({ apps: [app], installations: [installation] });

    expect(await githubApi.getConnection('org-1')).toEqual({
      apps: [app],
      installations: [installation],
    });
    expect(sent(request)).toEqual({ url: base, method: 'GET', body: undefined });
  });

  it('percent-encodes the organization id', async () => {
    const request = respond({ apps: [], installations: [] });

    await githubApi.getConnection('org 1/2');

    expect(sent(request).url).toBe('/api/organizations/org%201%2F2/github');
  });

  it('sends the session token and the device headers', async () => {
    session.setAccessToken('token-1');
    const request = respond({ apps: [], installations: [] });

    await githubApi.getConnection('org-1');

    const headers = new Headers(request.mock.calls[0][1]?.headers);
    expect(headers.get('Authorization')).toBe('Bearer token-1');
    expect(headers.get('X-Zone-Device')).toBeTruthy();
  });

  it('rejects a connection that does not match the schema', async () => {
    respond({ apps: 'none', installations: [] });

    await expect(githubApi.getConnection('org-1')).rejects.toThrow();
  });
});

describe('githubApi.registerApp', () => {
  const registration = {
    action: 'https://github.com/settings/apps/new?state=state-1',
    manifest: '{"name":"Zone"}',
    state: 'state-1',
  };

  it('registers an app on the signed-in account with a null owner', async () => {
    const request = respond(registration);

    expect(await githubApi.registerApp('org-1', null)).toEqual(registration);
    expect(sent(request)).toEqual({
      url: `${base}/apps`,
      method: 'POST',
      body: '{"owner":null}',
    });
  });

  it('registers an app on the named organization', async () => {
    const request = respond(registration);

    await githubApi.registerApp('org-1', 'acme');

    expect(sent(request)).toEqual({
      url: `${base}/apps`,
      method: 'POST',
      body: '{"owner":"acme"}',
    });
  });

  it('rejects a conflict with the reason the server gave', async () => {
    respond({ success: false, error: 'already registered' }, 409);

    const failure = await githubApi.registerApp('org-1', 'acme').catch((error) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect(failure.status).toBe(409);
    expect(failure.message).toContain('already registered');
    expect(failure.message).toStartWith('Failed to connect GitHub');
  });
});

describe('githubApi.createInstallation', () => {
  it('starts an installation and resolves to the GitHub install url', async () => {
    const url = 'https://github.com/apps/zone-acme/installations/new?state=state-2';
    const request = respond({ url });

    expect(await githubApi.createInstallation('org-1', 'app-1')).toBe(url);
    expect(sent(request)).toEqual({
      url: `${base}/apps/app-1/installations`,
      method: 'POST',
      body: undefined,
    });
  });

  it('percent-encodes the app id', async () => {
    const request = respond({ url: 'https://github.com/apps/zone-acme/installations/new' });

    await githubApi.createInstallation('org-1', 'app 1');

    expect(sent(request).url).toBe(`${base}/apps/app%201/installations`);
  });
});

describe('githubApi.syncInstallations', () => {
  it('resolves to undefined on a 204 without reading a body', async () => {
    const request = respond(null, 204);

    expect(await githubApi.syncInstallations('org-1')).toBeUndefined();
    expect(sent(request)).toEqual({
      url: `${base}/installations/sync`,
      method: 'POST',
      body: undefined,
    });
  });
});

describe('githubApi.removeInstallation', () => {
  for (const uninstall of [true, false]) {
    it(`removes an installation with uninstall=${uninstall}`, async () => {
      const request = respond(null, 204);

      expect(await githubApi.removeInstallation('org-1', 'inst-1', uninstall)).toBeUndefined();
      expect(sent(request)).toEqual({
        url: `${base}/installations/inst-1?uninstall=${uninstall}`,
        method: 'DELETE',
        body: undefined,
      });
    });
  }
});

describe('githubApi.deleteApp', () => {
  it('deletes the app and resolves to undefined', async () => {
    const request = respond(null, 204);

    expect(await githubApi.deleteApp('org-1', 'app-1')).toBeUndefined();
    expect(sent(request)).toEqual({
      url: `${base}/apps/app-1`,
      method: 'DELETE',
      body: undefined,
    });
  });
});

describe('githubApi.listRepositories', () => {
  it('asks for the first page at the default page size when given no search', async () => {
    const request = respond(repositories);

    expect(await githubApi.listRepositories('org-1')).toEqual(repositories);
    expect(sent(request)).toEqual({
      url: `${base}/repositories?page=1&per_page=50`,
      method: 'GET',
      body: undefined,
    });
  });

  it('leaves out an empty query and a false refresh', async () => {
    const request = respond(repositories);

    await githubApi.listRepositories('org-1', { query: '', refresh: false });

    expect(sent(request).url).toBe(`${base}/repositories?page=1&per_page=50`);
  });

  it('puts the query before the paging parameters', async () => {
    const request = respond(repositories);

    await githubApi.listRepositories('org-1', { query: 'zone' });

    expect(sent(request).url).toBe(`${base}/repositories?query=zone&page=1&per_page=50`);
  });

  it('orders every parameter as query, page, page size, refresh', async () => {
    const request = respond(repositories);

    await githubApi.listRepositories('org-1', {
      refresh: true,
      perPage: 20,
      page: 3,
      query: 'zone',
    });

    expect(sent(request).url).toBe(
      `${base}/repositories?query=zone&page=3&per_page=20&refresh=true`
    );
  });

  it('rejects a next page that is not a number', async () => {
    respond({ ...repositories, next_page: '2' });

    await expect(githubApi.listRepositories('org-1')).rejects.toThrow();
  });
});

describe('githubApi.listBranches', () => {
  it('reads the first page of branches by default', async () => {
    const request = respond(branches);

    expect(await githubApi.listBranches('org-1', 'inst-1', 'acme', 'zone')).toEqual(branches);
    expect(sent(request)).toEqual({
      url: `${base}/installations/inst-1/repositories/acme/zone/branches?page=1`,
      method: 'GET',
      body: undefined,
    });
  });

  it('percent-encodes an owner with a space and asks for the given page', async () => {
    const request = respond(branches);

    await githubApi.listBranches('org-1', 'inst-1', 'acme corp', 'zone', 2);

    expect(sent(request).url).toBe(
      `${base}/installations/inst-1/repositories/acme%20corp/zone/branches?page=2`
    );
  });
});

describe('githubApi errors', () => {
  const calls: [string, () => Promise<unknown>][] = [
    ['Failed to load the GitHub connection', () => githubApi.getConnection('org-1')],
    ['Failed to connect GitHub', () => githubApi.registerApp('org-1', null)],
    [
      'Failed to start the GitHub installation',
      () => githubApi.createInstallation('org-1', 'app-1'),
    ],
    ['Failed to refresh GitHub accounts', () => githubApi.syncInstallations('org-1')],
    [
      'Failed to remove the GitHub account',
      () => githubApi.removeInstallation('org-1', 'inst-1', true),
    ],
    ['Failed to remove the GitHub App', () => githubApi.deleteApp('org-1', 'app-1')],
    ['Failed to load repositories', () => githubApi.listRepositories('org-1')],
    ['Failed to load branches', () => githubApi.listBranches('org-1', 'inst-1', 'acme', 'zone')],
  ];

  for (const [action, call] of calls) {
    it(`names the action "${action}" when the server refuses`, async () => {
      respond({ error: 'upstream unavailable' }, 502);

      const failure = await call().catch((error) => error);

      expect(failure).toBeInstanceOf(ApiError);
      expect(failure.status).toBe(502);
      expect(failure.message).toBe(`${action}: upstream unavailable`);
    });
  }
});
