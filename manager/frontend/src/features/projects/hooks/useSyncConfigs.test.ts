import { renderHook, waitFor } from '@testing-library/react';
import { useSyncConfigs } from './useSyncConfigs';
import { projectsApi } from '../../../api/projects';
import type { SyncConfig, CreateSyncConfigRequest } from '../types';

jest.mock('../../../api/projects');

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
    jest.clearAllMocks();
  });

  it('should fetch sync configs on mount', async () => {
    (projectsApi.getSyncConfigs as jest.Mock).mockResolvedValue(mockSyncConfigs);

    const { result } = renderHook(() => useSyncConfigs('proj-1'));

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.configs).toEqual(mockSyncConfigs);
    expect(result.current.error).toBeNull();
    expect(projectsApi.getSyncConfigs).toHaveBeenCalledWith('proj-1');
  });

  it('should handle fetch error', async () => {
    const error = new Error('Failed to fetch');
    (projectsApi.getSyncConfigs as jest.Mock).mockRejectedValue(error);

    const { result } = renderHook(() => useSyncConfigs('proj-1'));

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

    (projectsApi.getSyncConfigs as jest.Mock).mockResolvedValue(mockSyncConfigs);
    (projectsApi.createSyncConfig as jest.Mock).mockResolvedValue(newConfig);

    const { result } = renderHook(() => useSyncConfigs('proj-1'));

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

    expect(projectsApi.createSyncConfig).toHaveBeenCalledWith('proj-1', createRequest);
  });

  it('should delete sync config', async () => {
    (projectsApi.getSyncConfigs as jest.Mock).mockResolvedValue(mockSyncConfigs);
    (projectsApi.deleteSyncConfig as jest.Mock).mockResolvedValue(undefined);

    const { result } = renderHook(() => useSyncConfigs('proj-1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.deleteSyncConfig('1');

    await waitFor(() => {
      expect(result.current.configs).not.toContainEqual(mockSyncConfigs[0]);
    });

    expect(projectsApi.deleteSyncConfig).toHaveBeenCalledWith('proj-1', '1');
  });

  it('should refetch sync configs', async () => {
    (projectsApi.getSyncConfigs as jest.Mock).mockResolvedValue(mockSyncConfigs);

    const { result } = renderHook(() => useSyncConfigs('proj-1'));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(projectsApi.getSyncConfigs).toHaveBeenCalledTimes(1);

    await result.current.refetch();

    expect(projectsApi.getSyncConfigs).toHaveBeenCalledTimes(2);
  });

  it('should not fetch if projectId is null', () => {
    const { result } = renderHook(() => useSyncConfigs(null));

    expect(result.current.configs).toEqual([]);
    expect(result.current.loading).toBe(false);
    expect(projectsApi.getSyncConfigs).not.toHaveBeenCalled();
  });
});
