import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';

const getConnection = mock();
const registerApp = mock();
const createInstallation = mock();
const syncInstallations = mock();
const removeInstallation = mock();
const deleteApp = mock();
const listRepositories = mock();
const listBranches = mock();

mock.module('../../../api/github', () => ({
  githubApi: {
    getConnection,
    registerApp,
    createInstallation,
    syncInstallations,
    removeInstallation,
    deleteApp,
    listRepositories,
    listBranches,
  },
}));

let useGitHubConnection: typeof import('./useGitHubConnection').useGitHubConnection;

beforeAll(async () => {
  ({ useGitHubConnection } = await import('./useGitHubConnection'));
});

afterAll(() => {
  mock.restore();
});

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

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

const connection = { apps: [app], installations: [installation] };

async function renderLoaded() {
  getConnection.mockResolvedValue(connection);
  const rendered = renderHook(() => useGitHubConnection('org-1'), { wrapper: createWrapper() });
  await waitFor(() => {
    expect(rendered.result.current.loading).toBe(false);
  });
  return rendered;
}

describe('useGitHubConnection', () => {
  beforeEach(() => {
    for (const method of [
      getConnection,
      registerApp,
      createInstallation,
      syncInstallations,
      removeInstallation,
      deleteApp,
      listRepositories,
      listBranches,
    ]) {
      method.mockReset();
    }
  });

  it('never loads anything without an organization', () => {
    const { result } = renderHook(() => useGitHubConnection(null), { wrapper: createWrapper() });

    expect(result.current.apps).toEqual([]);
    expect(result.current.installations).toEqual([]);
    expect(result.current.loading).toBe(false);
    expect(getConnection).not.toHaveBeenCalled();
  });

  it('loads the apps and installations of the organization', async () => {
    const { result } = await renderLoaded();

    expect(getConnection).toHaveBeenCalledWith('org-1');
    expect(result.current.apps).toEqual([app]);
    expect(result.current.installations).toEqual([installation]);
    expect(result.current.error).toBeNull();
  });

  it('reports why the connection failed to load', async () => {
    getConnection.mockRejectedValue(new Error('Failed to load the GitHub connection: 500'));

    const { result } = renderHook(() => useGitHubConnection('org-1'), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load the GitHub connection: 500');
    });
    expect(result.current.loading).toBe(false);
    expect(result.current.apps).toEqual([]);
  });

  it('reloads the connection on refresh', async () => {
    const { result } = await renderLoaded();
    const reads = getConnection.mock.calls.length;

    await result.current.refresh();

    expect(getConnection.mock.calls.length).toBeGreaterThan(reads);
  });

  it('reports that it is fetching while a refresh is in flight', async () => {
    const { result } = await renderLoaded();
    expect(result.current.fetching).toBe(false);
    getConnection.mockImplementation(() => new Promise(() => {}));

    void result.current.refresh();

    await waitFor(() => {
      expect(result.current.fetching).toBe(true);
    });
    expect(result.current.loading).toBe(false);
  });

  it('rejects every action without an organization instead of throwing', async () => {
    const { result } = renderHook(() => useGitHubConnection(null), { wrapper: createWrapper() });
    const actions: [string, () => Promise<unknown>][] = [
      ['registerApp', () => result.current.registerApp(null)],
      ['createInstallation', () => result.current.createInstallation('app-1')],
      ['syncInstallations', () => result.current.syncInstallations()],
      ['removeInstallation', () => result.current.removeInstallation('inst-1', true)],
      ['deleteApp', () => result.current.deleteApp('app-1')],
    ];

    for (const [name, action] of actions) {
      let pending: Promise<unknown> | undefined;
      expect(() => {
        pending = action();
      }, `${name} threw synchronously`).not.toThrow();
      await expect(pending, `${name} did not reject`).rejects.toThrow(
        'An organization is required to manage GitHub'
      );
    }
    expect(registerApp).not.toHaveBeenCalled();
    expect(createInstallation).not.toHaveBeenCalled();
  });

  it('registers an app and hands back the registration', async () => {
    const registration = {
      action: 'https://github.com/organizations/acme/settings/apps/new?state=state-1',
      manifest: '{"name":"Zone"}',
      state: 'state-1',
    };
    registerApp.mockResolvedValue(registration);
    const { result } = await renderLoaded();

    expect(await result.current.registerApp('acme')).toEqual(registration);
    expect(await result.current.registerApp(null)).toEqual(registration);

    expect(registerApp).toHaveBeenNthCalledWith(1, 'org-1', 'acme');
    expect(registerApp).toHaveBeenNthCalledWith(2, 'org-1', null);
  });

  it('starts an installation and hands back its url', async () => {
    const url = 'https://github.com/apps/zone-acme/installations/new?state=state-2';
    createInstallation.mockResolvedValue(url);
    const { result } = await renderLoaded();

    expect(await result.current.createInstallation('app-1')).toBe(url);
    expect(createInstallation).toHaveBeenCalledWith('org-1', 'app-1');
  });

  it('rereads the connection after syncing installations', async () => {
    syncInstallations.mockResolvedValue(undefined);
    const { result } = await renderLoaded();
    const reads = getConnection.mock.calls.length;

    await result.current.syncInstallations();

    expect(syncInstallations).toHaveBeenCalledWith('org-1');
    await waitFor(() => {
      expect(getConnection.mock.calls.length).toBeGreaterThan(reads);
    });
  });

  for (const uninstall of [true, false]) {
    it(`rereads the connection after removing an installation with uninstall=${uninstall}`, async () => {
      removeInstallation.mockResolvedValue(undefined);
      const { result } = await renderLoaded();
      const reads = getConnection.mock.calls.length;

      await result.current.removeInstallation('inst-1', uninstall);

      expect(removeInstallation).toHaveBeenCalledWith('org-1', 'inst-1', uninstall);
      await waitFor(() => {
        expect(getConnection.mock.calls.length).toBeGreaterThan(reads);
      });
    });
  }

  it('rereads the connection after deleting an app', async () => {
    deleteApp.mockResolvedValue(undefined);
    const { result } = await renderLoaded();
    const reads = getConnection.mock.calls.length;

    await result.current.deleteApp('app-1');

    expect(deleteApp).toHaveBeenCalledWith('org-1', 'app-1');
    await waitFor(() => {
      expect(getConnection.mock.calls.length).toBeGreaterThan(reads);
    });
  });
});
