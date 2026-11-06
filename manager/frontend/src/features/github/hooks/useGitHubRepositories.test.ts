import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';

const listRepositories = mock();

mock.module('../../../api/github', () => ({
  githubApi: {
    getConnection: mock(),
    registerApp: mock(),
    createInstallation: mock(),
    syncInstallations: mock(),
    removeInstallation: mock(),
    deleteApp: mock(),
    listRepositories,
    listBranches: mock(),
  },
}));

let useGitHubRepositories: typeof import('./useGitHubRepositories').useGitHubRepositories;
let githubQueryKeys: typeof import('./githubQueryKeys').githubQueryKeys;

beforeAll(async () => {
  ({ useGitHubRepositories } = await import('./useGitHubRepositories'));
  ({ githubQueryKeys } = await import('./githubQueryKeys'));
});

afterAll(() => {
  mock.restore();
});

function createQueryClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
}

function createWrapper(queryClient = createQueryClient()) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

function deferred<Value>() {
  let resolve: (value: Value) => void = () => {};
  const promise = new Promise<Value>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

function repository(name: string) {
  return {
    installation_id: 'inst-1',
    id: name.length,
    owner: 'acme',
    name,
    full_name: `acme/${name}`,
    private: false,
    archived: false,
    default_branch: 'main',
    html_url: `https://github.com/acme/${name}`,
  };
}

const installations = [
  {
    id: 'inst-1',
    account_login: 'acme',
    html_url: 'https://github.com/organizations/acme/settings/installations/5001',
    error: null,
  },
];

const first = {
  repositories: [repository('zone'), repository('zone-ui')],
  installations,
  total: 3,
  next_page: 2,
};

const second = {
  repositories: [repository('website')],
  installations,
  total: 3,
  next_page: null,
};

describe('useGitHubRepositories', () => {
  beforeEach(() => {
    listRepositories.mockReset();
  });

  it('reads the first page of every installation at the repository page size', async () => {
    listRepositories.mockResolvedValue(first);

    const { result } = renderHook(() => useGitHubRepositories('org-1', ''), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.repositories).toEqual(first.repositories);
    });
    expect(listRepositories).toHaveBeenCalledTimes(1);
    expect(listRepositories).toHaveBeenCalledWith('org-1', { query: '', page: 1, perPage: 50 });
    expect(result.current.installations).toEqual(installations);
    expect(result.current.total).toBe(3);
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('narrows the search to one installation when given one', async () => {
    listRepositories.mockResolvedValue(first);

    const { result } = renderHook(() => useGitHubRepositories('org-1', 'zone', 'inst-1'), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(listRepositories).toHaveBeenCalledWith('org-1', {
      query: 'zone',
      page: 1,
      perPage: 50,
      installationId: 'inst-1',
    });
  });

  it('appends the next page on loadMore until the server reports no more', async () => {
    listRepositories.mockResolvedValueOnce(first).mockResolvedValueOnce(second);

    const { result } = renderHook(() => useGitHubRepositories('org-1', ''), {
      wrapper: createWrapper(),
    });
    await waitFor(() => {
      expect(result.current.hasMore).toBe(true);
    });

    await result.current.loadMore();

    expect(listRepositories).toHaveBeenLastCalledWith('org-1', { query: '', page: 2, perPage: 50 });
    await waitFor(() => {
      expect(result.current.repositories).toEqual([...first.repositories, ...second.repositories]);
    });
    expect(result.current.hasMore).toBe(false);
    expect(result.current.loadingMore).toBe(false);
  });

  it('has no more to load when the first page is the last', async () => {
    listRepositories.mockResolvedValue({ ...first, next_page: null });

    const { result } = renderHook(() => useGitHubRepositories('org-1', ''), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(result.current.hasMore).toBe(false);
  });

  it('keeps the previous repositories visible while a new query loads', async () => {
    const pending = deferred<typeof second>();
    listRepositories.mockResolvedValueOnce(first).mockReturnValueOnce(pending.promise);

    const { result, rerender } = renderHook(
      ({ query }: { query: string }) => useGitHubRepositories('org-1', query),
      { wrapper: createWrapper(), initialProps: { query: '' } }
    );
    await waitFor(() => {
      expect(result.current.repositories).toEqual(first.repositories);
    });

    rerender({ query: 'web' });

    await waitFor(() => {
      expect(listRepositories).toHaveBeenCalledTimes(2);
    });
    expect(listRepositories).toHaveBeenLastCalledWith('org-1', {
      query: 'web',
      page: 1,
      perPage: 50,
    });
    expect(result.current.repositories).toEqual(first.repositories);
    expect(result.current.loading).toBe(false);

    pending.resolve(second);

    await waitFor(() => {
      expect(result.current.repositories).toEqual(second.repositories);
    });
  });

  it('refreshes from GitHub and replaces every loaded page with the fresh first page', async () => {
    const refreshed = {
      repositories: [repository('zone'), repository('docs')],
      installations,
      total: 2,
      next_page: null,
    };
    listRepositories
      .mockResolvedValueOnce(first)
      .mockResolvedValueOnce(second)
      .mockResolvedValueOnce(refreshed);
    const queryClient = createQueryClient();

    const { result } = renderHook(() => useGitHubRepositories('org-1', ''), {
      wrapper: createWrapper(queryClient),
    });
    await waitFor(() => {
      expect(result.current.hasMore).toBe(true);
    });
    await result.current.loadMore();
    await waitFor(() => {
      expect(result.current.repositories).toHaveLength(3);
    });

    await result.current.refresh();

    expect(listRepositories).toHaveBeenLastCalledWith('org-1', {
      query: '',
      page: 1,
      perPage: 50,
      refresh: true,
    });
    await waitFor(() => {
      expect(result.current.repositories).toEqual(refreshed.repositories);
    });
    expect(result.current.total).toBe(2);
    expect(result.current.hasMore).toBe(false);
    expect(queryClient.getQueryData(githubQueryKeys.repositories('org-1', '', null))).toEqual({
      pages: [refreshed],
      pageParams: [1],
    });
  });

  it('refreshes within the chosen installation', async () => {
    listRepositories.mockResolvedValue(first);

    const { result } = renderHook(() => useGitHubRepositories('org-1', 'zone', 'inst-1'), {
      wrapper: createWrapper(),
    });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.refresh();

    expect(listRepositories).toHaveBeenLastCalledWith('org-1', {
      query: 'zone',
      page: 1,
      perPage: 50,
      refresh: true,
      installationId: 'inst-1',
    });
  });

  it('rereads the current query on refetch without asking GitHub again', async () => {
    listRepositories.mockResolvedValue(first);

    const { result } = renderHook(() => useGitHubRepositories('org-1', ''), {
      wrapper: createWrapper(),
    });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.refetch();

    expect(listRepositories).toHaveBeenCalledTimes(2);
    expect(listRepositories).toHaveBeenLastCalledWith('org-1', { query: '', page: 1, perPage: 50 });
  });

  it('reports why the repositories failed to load', async () => {
    listRepositories.mockRejectedValue(new Error('Failed to load repositories: 502'));

    const { result } = renderHook(() => useGitHubRepositories('org-1', ''), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load repositories: 502');
    });
    expect(result.current.repositories).toEqual([]);
  });

  it('reads nothing without an organization, and refresh stays a no-op', async () => {
    const { result } = renderHook(() => useGitHubRepositories(null, ''), {
      wrapper: createWrapper(),
    });

    await result.current.refresh();
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(result.current.repositories).toEqual([]);
    expect(result.current.installations).toEqual([]);
    expect(result.current.hasMore).toBe(false);
    expect(listRepositories).not.toHaveBeenCalled();
  });
});
