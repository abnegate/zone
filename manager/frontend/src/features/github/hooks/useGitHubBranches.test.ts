import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { ApiError } from '../../../api/ApiError';

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

  it('reads the branches of the repository through its installation', async () => {
    listBranches.mockResolvedValue({ branches, next_page: null });

    const { result } = renderHook(() => useGitHubBranches('org-1', repository), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.branches).toEqual(branches);
    });
    expect(listBranches).toHaveBeenCalledWith('org-1', 'inst-1', 'acme', 'zone', 1);
    expect(listBranches).toHaveBeenCalledTimes(1);
    expect(result.current.loading).toBe(false);
    expect(result.current.failed).toBe(false);
  });

  it('follows next_page until the last page so a repository is not capped at one page', async () => {
    const release = { name: 'release', protected: false };
    listBranches
      .mockResolvedValueOnce({ branches, next_page: 2 })
      .mockResolvedValueOnce({ branches: [release], next_page: null });

    const { result } = renderHook(() => useGitHubBranches('org-1', repository), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.branches).toEqual([...branches, release]);
    });
    expect(listBranches).toHaveBeenNthCalledWith(1, 'org-1', 'inst-1', 'acme', 'zone', 1);
    expect(listBranches).toHaveBeenNthCalledWith(2, 'org-1', 'inst-1', 'acme', 'zone', 2);
    expect(listBranches).toHaveBeenCalledTimes(2);
  });

  it('stops paging when the server repeats a page instead of advancing', async () => {
    listBranches.mockResolvedValue({ branches, next_page: 1 });

    const { result } = renderHook(() => useGitHubBranches('org-1', repository), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.branches).toEqual(branches);
    });
    expect(listBranches).toHaveBeenCalledTimes(1);
  });

  it('reports that the branches failed to load', async () => {
    listBranches.mockRejectedValue(new ApiError('Failed to load branches: 502', 502));

    const { result } = renderHook(() => useGitHubBranches('org-1', repository), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.failed).toBe(true);
    });
    expect(result.current.branches).toEqual([]);
    expect(result.current.loading).toBe(false);
  });

  it('treats a missing installation or repository as no branches rather than a failure', async () => {
    listBranches.mockRejectedValue(new ApiError('Failed to load branches: 404', 404));

    const { result } = renderHook(() => useGitHubBranches('org-1', repository), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(listBranches).toHaveBeenCalledTimes(1);
    expect(result.current.failed).toBe(false);
    expect(result.current.branches).toEqual([]);
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
