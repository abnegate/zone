import { beforeEach, describe, expect, it, mock } from 'bun:test';
import type { CreateSyncConfigRequest, SyncConfig } from '../features/projects/types';
import { client } from './client';

describe('Client - Sync Configuration API', () => {
  let mockFetch: ReturnType<typeof mock>;

  beforeEach(() => {
    mock.clearAllMocks();
    mockFetch = mock();
    global.fetch = mockFetch as typeof fetch;
    client.setAccessToken('test-token');
  });

  describe('getSyncConfigs', () => {
    it('should fetch sync configs for a project', async () => {
      const mockConfigs: SyncConfig[] = [
        {
          id: 'config-1',
          project_id: 'proj-1',
          provider: 'github',
          direction: 'bidirectional',
          external_repo_url: 'https://github.com/user/repo',
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
        },
      ];

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ configs: mockConfigs }),
      });

      const result = await client.getSyncConfigs('proj-1');

      expect(global.fetch).toHaveBeenCalledWith(
        '/api/projects/proj-1/sync',
        expect.objectContaining({
          headers: expect.objectContaining({
            Authorization: 'Bearer test-token',
          }),
        })
      );
      expect(result).toEqual(mockConfigs);
    });

    it('should handle fetch errors', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 404,
        json: async () => ({ message: 'Project not found' }),
      });

      await expect(client.getSyncConfigs('proj-1')).rejects.toThrow('Project not found');
    });
  });

  describe('createSyncConfig', () => {
    it('should create a GitHub sync config', async () => {
      const request: CreateSyncConfigRequest = {
        provider: 'github',
        direction: 'bidirectional',
        external_repo_url: 'https://github.com/user/repo',
      };

      const mockConfig: SyncConfig = {
        id: 'config-1',
        project_id: 'proj-1',
        ...request,
        is_active: true,
        created_at: '2024-01-01T00:00:00Z',
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ config: mockConfig }),
      });

      const result = await client.createSyncConfig('proj-1', request);

      expect(global.fetch).toHaveBeenCalledWith(
        '/api/projects/proj-1/sync',
        expect.objectContaining({
          method: 'POST',
          headers: expect.objectContaining({
            Authorization: 'Bearer test-token',
            'Content-Type': 'application/json',
          }),
          body: JSON.stringify(request),
        })
      );
      expect(result).toEqual(mockConfig);
    });

    it('should create a Linear sync config', async () => {
      const request: CreateSyncConfigRequest = {
        provider: 'linear',
        direction: 'inbound',
        external_project_id: 'LINEAR-123',
      };

      const mockConfig: SyncConfig = {
        id: 'config-2',
        project_id: 'proj-1',
        ...request,
        is_active: true,
        created_at: '2024-01-01T00:00:00Z',
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ config: mockConfig }),
      });

      const result = await client.createSyncConfig('proj-1', request);

      expect(result).toEqual(mockConfig);
      expect(result.provider).toBe('linear');
      expect(result.external_project_id).toBe('LINEAR-123');
    });

    it('should handle validation errors', async () => {
      const request: CreateSyncConfigRequest = {
        provider: 'github',
        direction: 'bidirectional',
        // Missing external_repo_url
      };

      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 400,
        json: async () => ({ message: 'Validation error' }),
      });

      await expect(client.createSyncConfig('proj-1', request)).rejects.toThrow('Validation error');
    });
  });

  describe('deleteSyncConfig', () => {
    it('should delete a sync config', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
      });

      await client.deleteSyncConfig('proj-1', 'config-1');

      expect(global.fetch).toHaveBeenCalledWith(
        '/api/projects/proj-1/sync/config-1',
        expect.objectContaining({
          method: 'DELETE',
          headers: expect.objectContaining({
            Authorization: 'Bearer test-token',
          }),
        })
      );
    });

    it('should handle delete errors', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 403,
        json: async () => ({ message: 'Forbidden' }),
      });

      await expect(client.deleteSyncConfig('proj-1', 'config-1')).rejects.toThrow('Forbidden');
    });
  });
});
