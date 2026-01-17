/**
 * Tests for useSources hook
 */

import { renderHook, waitFor } from '@testing-library/react';
import { act } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { Source } from '../types';
import { createElement } from 'react';
import type { ReactNode } from 'react';

const mockGetSources = mock();
const mockCreateSource = mock();
const mockUpdateSource = mock();
const mockDeleteSource = mock();
const mockVerifySource = mock();
const mockGetSource = mock();

mock.module('../../../api/sources', () => ({
  sourcesApi: {
    getSources: mockGetSources,
    createSource: mockCreateSource,
    updateSource: mockUpdateSource,
    deleteSource: mockDeleteSource,
    verifySource: mockVerifySource,
    getSource: mockGetSource,
  },
}));

// Mock useWorkspace to provide a test workspace
mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: { id: 'test-workspace-id', name: 'Test Workspace' },
    currentOrganization: { id: 'test-org-id', name: 'Test Org' },
    workspaces: [],
    organizations: [],
    loading: false,
    error: null,
    setCurrentWorkspace: mock(),
    setCurrentOrganization: mock(),
    refreshWorkspaces: mock(),
    refreshOrganizations: mock(),
  }),
}));

let useSources: typeof import('./useSources').useSources;

beforeAll(async () => {
  ({ useSources } = await import('./useSources'));
});

afterAll(() => {
  mock.restore();
});

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
};

describe('useSources', () => {
  const mockSources: Source[] = [
    {
      id: '1',
      name: 'Test GitHub',
      source_type: 'github',
      category: 'file',
      config: { owner: 'test', repo: 'repo' },
      description: null,
      url: 'https://github.com/test/repo',
      is_active: true,
      last_verified_at: null,
      last_error: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
    {
      id: '2',
      name: 'Test GitLab',
      source_type: 'gitlab',
      category: 'file',
      config: { project_id: '123' },
      description: 'Test description',
      url: 'https://gitlab.com/test/project',
      is_active: false,
      last_verified_at: '2024-01-01T00:00:00Z',
      last_error: 'Connection failed',
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
  ];

  beforeEach(() => {
    mockGetSources.mockReset();
    mockCreateSource.mockReset();
    mockUpdateSource.mockReset();
    mockDeleteSource.mockReset();
    mockVerifySource.mockReset();
    mockGetSource.mockReset();
  });

  it('should fetch sources on mount', async () => {
    mockGetSources.mockResolvedValue(mockSources);

    const { result } = renderHook(() => useSources(), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);
    expect(result.current.sources).toEqual([]);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.sources).toEqual(mockSources);
    expect(result.current.error).toBeNull();
    expect(mockGetSources).toHaveBeenCalledTimes(1);
  });

  it('should handle fetch error', async () => {
    const errorMessage = 'Failed to fetch sources';
    mockGetSources.mockRejectedValue(new Error(errorMessage));

    const { result } = renderHook(() => useSources(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.sources).toEqual([]);
    expect(result.current.error).toBe(errorMessage);
  });

  it('should filter sources by type', async () => {
    mockGetSources.mockResolvedValue(mockSources);

    const { result } = renderHook(() => useSources({ type: 'github' }), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(mockGetSources).toHaveBeenCalledWith('test-workspace-id', 'github', false);
  });

  it('should filter active sources only', async () => {
    mockGetSources.mockResolvedValue([mockSources[0]]);

    const { result } = renderHook(() => useSources({ activeOnly: true }), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(mockGetSources).toHaveBeenCalledWith('test-workspace-id', undefined, true);
  });

  it('should create source', async () => {
    const newSource: Source = { ...mockSources[0], id: '3', name: 'New Source' };
    mockGetSources.mockResolvedValue(mockSources);
    mockCreateSource.mockResolvedValue(newSource);

    const { result } = renderHook(() => useSources(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.createSource({
        name: 'New Source',
        source_type: 'github',
        config: { owner: 'test', repo: 'new' },
      });
    });

    expect(result.current.sources).toContainEqual(newSource);
  });

  it('should update source', async () => {
    const updatedSource: Source = { ...mockSources[0], name: 'Updated Name' };
    mockGetSources.mockResolvedValue(mockSources);
    mockUpdateSource.mockResolvedValue(updatedSource);

    const { result } = renderHook(() => useSources(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.updateSource('1', { name: 'Updated Name' });
    });

    expect(result.current.sources.find((s) => s.id === '1')?.name).toBe('Updated Name');
  });

  it('should delete source', async () => {
    mockGetSources.mockResolvedValue(mockSources);
    mockDeleteSource.mockResolvedValue();

    const { result } = renderHook(() => useSources(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.sources).toHaveLength(2);

    await act(async () => {
      await result.current.deleteSource('1');
    });

    expect(result.current.sources).toHaveLength(1);
    expect(result.current.sources.find((s) => s.id === '1')).toBeUndefined();
  });

  it('should verify source', async () => {
    const verifiedSource: Source = {
      ...mockSources[0],
      last_verified_at: '2024-01-02T00:00:00Z',
    };
    mockGetSources.mockResolvedValue(mockSources);
    mockVerifySource.mockResolvedValue({
      success: true,
      message: 'Verified successfully',
    });
    mockGetSource.mockResolvedValue(verifiedSource);

    const { result } = renderHook(() => useSources(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    let verifyResult: { success: boolean; message: string } | undefined;
    await act(async () => {
      verifyResult = await result.current.verifySource('1');
    });

    expect(verifyResult).toEqual({ success: true, message: 'Verified successfully' });
    expect(result.current.sources.find((s) => s.id === '1')?.last_verified_at).toBe(
      '2024-01-02T00:00:00Z'
    );
  });

  it('should refresh sources', async () => {
    mockGetSources.mockResolvedValue(mockSources);

    const { result } = renderHook(() => useSources(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(mockGetSources).toHaveBeenCalledTimes(1);

    await act(async () => {
      await result.current.refresh();
    });

    expect(mockGetSources).toHaveBeenCalledTimes(2);
  });
});
