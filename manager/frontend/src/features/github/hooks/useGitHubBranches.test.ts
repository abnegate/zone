import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';

const listBranches = mock();

mock.module('../../../api/github', () => ({
  githubApi: {
    getConnection: mock(),
    registerApp: mock(),
    createInstallation: mock(),
    syncInstallations: mock(),
    removeInstallation: mock(),
    deleteApp: mock(),
    listRepositories: mock(),
    listBranches,
  },
}));

let useGitHubBranches: typeof import('./useGitHubBranches').useGitHubBranches;

beforeAll(async () => {
  ({ useGitHubBranches } = await import('./useGitHubBranches'));
});

afterAll(() => {
  mock.restore();
});

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

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

const branches = [
  { name: 'main', protected: true },
  { name: 'feature/picker', protected: false },
];

describe('useGitHubBranches', () => {
  beforeEach(() => {
    listBranches.mockReset();
  });

  it('reads the first page of branches of the repository through its installation', async () => {
    listBranches.mockResolvedValue({ branches, next_page: 2 });

    const { result } = renderHook(() => useGitHubBranches('org-1', repository), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.branches).toEqual(branches);
    });
    expect(listBranches).toHaveBeenCalledWith('org-1', 'inst-1', 'acme', 'zone', 1);
    expect(listBranches).toHaveBeenCalledTimes(1);
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('reports why the branches failed to load', async () => {
    listBranches.mockRejectedValue(new Error('Failed to load branches: 404'));

    const { result } = renderHook(() => useGitHubBranches('org-1', repository), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load branches: 404');
    });
    expect(result.current.branches).toEqual([]);
    expect(result.current.loading).toBe(false);
  });

  it('reads nothing until a repository is chosen', async () => {
    const { result } = renderHook(() => useGitHubBranches('org-1', null), {
      wrapper: createWrapper(),
    });

    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(result.current.branches).toEqual([]);
    expect(listBranches).not.toHaveBeenCalled();
  });

  it('reads nothing without an organization', async () => {
    const { result } = renderHook(() => useGitHubBranches(null, repository), {
      wrapper: createWrapper(),
    });

    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(result.current.branches).toEqual([]);
    expect(listBranches).not.toHaveBeenCalled();
  });
});
