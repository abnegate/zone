import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { SyncConfig, CreateSyncConfigRequest } from '../types';
import { createElement } from 'react';
import type { ReactNode } from 'react';

const mockGetSyncConfigs = mock();
const mockCreateSyncConfig = mock();
const mockDeleteSyncConfig = mock();

mock.module('../../../api/projects', () => ({
  projectsApi: {
    getSyncConfigs: mockGetSyncConfigs,
    createSyncConfig: mockCreateSyncConfig,
    deleteSyncConfig: mockDeleteSyncConfig,
  },
}));

let useSyncConfigs: typeof import('./useSyncConfigs').useSyncConfigs;

beforeAll(async () => {
  ({ useSyncConfigs } = await import('./useSyncConfigs'));
});

afterAll(() => {
  mock.restore();
});

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
};

const mockSyncConfigs: SyncConfig[] = [
  {
    id: '1',
    project_id: 'proj-1',
    provider: 'github',
    direction: 'bidirectional',
    external_repo_url: 'https://github.com/owner/repo',
    is_active: true,
    created_at: '2024-01-01T00:00:00Z',
  },
  {
    id: '2',
    project_id: 'proj-1',
    provider: 'linear',
    direction: 'inbound',
    external_project_id: 'LINEAR-123',
    is_active: true,
    created_at: '2024-01-02T00:00:00Z',
  },
];

describe('useSyncConfigs', () => {
  beforeEach(() => {
    mockGetSyncConfigs.mockReset();
    mockCreateSyncConfig.mockReset();
    mockDeleteSyncConfig.mockReset();
  });

  it('should fetch sync configs on mount', async () => {
    mockGetSyncConfigs.mockResolvedValue(mockSyncConfigs);

    const { result } = renderHook(() => useSyncConfigs('proj-1'), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.configs).toEqual(mockSyncConfigs);
    expect(result.current.error).toBeNull();
    expect(mockGetSyncConfigs).toHaveBeenCalledWith('proj-1');
  });

  it('should handle fetch error', async () => {
    const error = new Error('Failed to fetch');
    mockGetSyncConfigs.mockRejectedValue(error);

    const { result } = renderHook(() => useSyncConfigs('proj-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.configs).toEqual([]);
    expect(result.current.error).toBe('Failed to fetch');
  });

  it('should create sync config', async () => {
    const newConfig: SyncConfig = {
      id: '3',
      project_id: 'proj-1',
      provider: 'github',
      direction: 'outbound',
      external_repo_url: 'https://github.com/owner/other-repo',
      is_active: true,
      created_at: '2024-01-03T00:00:00Z',
    };

    // First call returns initial configs, second call (after refetch) returns updated list
    mockGetSyncConfigs
      .mockResolvedValueOnce(mockSyncConfigs)
      .mockResolvedValueOnce([...mockSyncConfigs, newConfig]);
    mockCreateSyncConfig.mockResolvedValue(newConfig);

    const { result } = renderHook(() => useSyncConfigs('proj-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const createRequest: CreateSyncConfigRequest = {
      provider: 'github',
      direction: 'outbound',
      external_repo_url: 'https://github.com/owner/other-repo',
    };

    await result.current.createSyncConfig(createRequest);

    await waitFor(() => {
      expect(result.current.configs).toContainEqual(newConfig);
    });

    expect(mockCreateSyncConfig).toHaveBeenCalledWith('proj-1', createRequest);
  });

  it('should delete sync config', async () => {
    // First call returns initial configs, second call (after refetch) returns list without deleted config
    mockGetSyncConfigs
      .mockResolvedValueOnce(mockSyncConfigs)
      .mockResolvedValueOnce([mockSyncConfigs[1]]);
    mockDeleteSyncConfig.mockResolvedValue(undefined);

    const { result } = renderHook(() => useSyncConfigs('proj-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.deleteSyncConfig('1');

    await waitFor(() => {
      expect(result.current.configs).not.toContainEqual(mockSyncConfigs[0]);
    });

    expect(mockDeleteSyncConfig).toHaveBeenCalledWith('proj-1', '1');
  });

  it('should refetch sync configs', async () => {
    mockGetSyncConfigs.mockResolvedValue(mockSyncConfigs);

    const { result } = renderHook(() => useSyncConfigs('proj-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(mockGetSyncConfigs).toHaveBeenCalledTimes(1);

    await result.current.refetch();

    expect(mockGetSyncConfigs).toHaveBeenCalledTimes(2);
  });

  it('should not fetch if projectId is null', () => {
    const { result } = renderHook(() => useSyncConfigs(null), { wrapper: createWrapper() });

    expect(result.current.configs).toEqual([]);
    expect(result.current.loading).toBe(false);
    expect(mockGetSyncConfigs).not.toHaveBeenCalled();
  });
});
