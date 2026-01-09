import { useCallback, useEffect, useState } from 'react';
import { projectsApi } from '../../../api/projects';
import type { SyncConfig, CreateSyncConfigRequest } from '../types';

export function useSyncConfigs(projectId: string | null) {
  const [configs, setConfigs] = useState<SyncConfig[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchSyncConfigs = useCallback(async () => {
    if (!projectId) {
      setConfigs([]);
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const data = await projectsApi.getSyncConfigs(projectId);
      setConfigs(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load sync configs');
      setConfigs([]);
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  useEffect(() => {
    fetchSyncConfigs();
  }, [fetchSyncConfigs]);

  const createSyncConfig = useCallback(
    async (request: CreateSyncConfigRequest): Promise<SyncConfig> => {
      if (!projectId) {
        throw new Error('Project ID is required');
      }
      const config = await projectsApi.createSyncConfig(projectId, request);
      setConfigs((prev) => [...prev, config]);
      return config;
    },
    [projectId]
  );

  const deleteSyncConfig = useCallback(
    async (configId: string): Promise<void> => {
      if (!projectId) {
        throw new Error('Project ID is required');
      }
      await projectsApi.deleteSyncConfig(projectId, configId);
      setConfigs((prev) => prev.filter((c) => c.id !== configId));
    },
    [projectId]
  );

  const refetch = useCallback(() => {
    return fetchSyncConfigs();
  }, [fetchSyncConfigs]);

  return {
    configs,
    loading,
    error,
    createSyncConfig,
    deleteSyncConfig,
    refetch,
  };
}
