import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import type { CreateSyncConfigRequest, SyncConfig } from '../types';

const mockGetSyncConfigs = mock();
const mockCreateSyncConfig = mock();
const mockDeleteSyncConfig = mock();
const mockSetWebhookSecret = mock();

mock.module('../../../api/projects', () => ({
  projectsApi: {
    getSyncConfigs: mockGetSyncConfigs,
    createSyncConfig: mockCreateSyncConfig,
    deleteSyncConfig: mockDeleteSyncConfig,
    setWebhookSecret: mockSetWebhookSecret,
  },
}));

let useSyncConfigs: typeof import('./useSyncConfigs').useSyncConfigs;

beforeAll(async () => {
  ({ useSyncConfigs } = await import('./useSyncConfigs'));
});

afterAll(() => {
  mock.restore();
});

const createQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });

const createWrapper = (queryClient = createQueryClient()) => {
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
    webhook_secret_issued_by_zone: true,
    created_at: '2024-01-01T00:00:00Z',
  },
  {
    id: '2',
    project_id: 'proj-1',
    provider: 'linear',
    direction: 'inbound',
    external_project_id: 'LINEAR-123',
    is_active: true,
    webhook_secret_issued_by_zone: false,
    created_at: '2024-01-02T00:00:00Z',
  },
];

describe('useSyncConfigs', () => {
  beforeEach(() => {
    mockGetSyncConfigs.mockReset();
    mockCreateSyncConfig.mockReset();
    mockDeleteSyncConfig.mockReset();
    mockSetWebhookSecret.mockReset();
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
      webhook_secret_issued_by_zone: true,
      created_at: '2024-01-03T00:00:00Z',
    };

    // First call returns initial configs, second call (after refetch) returns updated list
    mockGetSyncConfigs
      .mockResolvedValueOnce(mockSyncConfigs)
      .mockResolvedValueOnce([...mockSyncConfigs, newConfig]);
    mockCreateSyncConfig.mockResolvedValue({ config: newConfig, webhookSecret: 'e'.repeat(64) });

    const { result } = renderHook(() => useSyncConfigs('proj-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const createRequest: CreateSyncConfigRequest = {
      provider: 'github',
      direction: 'outbound',
      external_repo_url: 'https://github.com/owner/other-repo',
    };

    const created = await result.current.createSyncConfig(createRequest);

    expect(created).toEqual({ config: newConfig, webhookSecret: 'e'.repeat(64) });

    await waitFor(() => {
      expect(result.current.configs).toContainEqual(newConfig);
    });

    expect(mockCreateSyncConfig).toHaveBeenCalledWith('proj-1', createRequest);
  });

  it('keeps no generated secret in the query or mutation caches', async () => {
    const secret = 'f'.repeat(64);
    const queryClient = createQueryClient();
    mockGetSyncConfigs.mockResolvedValue(mockSyncConfigs);
    mockCreateSyncConfig.mockResolvedValue({ config: mockSyncConfigs[0], webhookSecret: secret });
    mockSetWebhookSecret.mockResolvedValue({ config: mockSyncConfigs[0], webhookSecret: secret });

    const { result } = renderHook(() => useSyncConfigs('proj-1'), {
      wrapper: createWrapper(queryClient),
    });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.createSyncConfig({
      provider: 'github',
      direction: 'outbound',
      external_repo_url: 'https://github.com/owner/repo',
    });
    await result.current.setWebhookSecret('1');

    await waitFor(() => {
      const mutations = queryClient.getMutationCache().getAll();
      expect(JSON.stringify(mutations.map((mutation) => mutation.state.data))).not.toContain(
        secret
      );
    });
    const queries = queryClient.getQueryCache().getAll();
    expect(JSON.stringify(queries.map((query) => query.state.data))).not.toContain(secret);
  });

  it('asks for a generated secret or sets a supplied one, then rereads the configs', async () => {
    mockGetSyncConfigs.mockResolvedValue(mockSyncConfigs);
    mockSetWebhookSecret.mockResolvedValue({ config: mockSyncConfigs[1], webhookSecret: null });

    const { result } = renderHook(() => useSyncConfigs('proj-1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.setWebhookSecret('1');
    const set = await result.current.setWebhookSecret('2', 'lin_wh_0123456789abcdef');

    expect(mockSetWebhookSecret).toHaveBeenNthCalledWith(1, 'proj-1', '1', undefined);
    expect(mockSetWebhookSecret).toHaveBeenNthCalledWith(
      2,
      'proj-1',
      '2',
      'lin_wh_0123456789abcdef'
    );
    expect(set.webhookSecret).toBeNull();
    await waitFor(() => {
      expect(mockGetSyncConfigs.mock.calls.length).toBeGreaterThanOrEqual(3);
    });
  });

  it('rereads the configs when a secret change is refused, so a stale card catches up', async () => {
    mockGetSyncConfigs.mockResolvedValue(mockSyncConfigs);
    mockSetWebhookSecret.mockRejectedValue(
      new Error(
        'The sync configuration changed while this request was replacing its webhook secret; reload and try again'
      )
    );

    const { result } = renderHook(() => useSyncConfigs('proj-1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    const reads = mockGetSyncConfigs.mock.calls.length;

    await expect(result.current.setWebhookSecret('1')).rejects.toThrow('reload and try again');

    await waitFor(() => {
      expect(mockGetSyncConfigs.mock.calls.length).toBeGreaterThan(reads);
    });
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
