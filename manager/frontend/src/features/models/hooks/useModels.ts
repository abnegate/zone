import { useCallback, useEffect, useRef, useState } from 'react';
import { ApiError } from '../../../api/ApiError';
import { modelsApi } from '../../../api/models';
import { useAuth } from '../../../features/auth';
import type { DiskUsage, InstalledModel, ProviderErrors } from '../types';

/**
 * The installed models, or with `workspaceId` the models a chat in that
 * workspace can run: only those its AI settings save when they send its
 * completions to an endpoint of their own.
 */
export function useModels(workspaceId?: string) {
  const { isAuthenticated, isLoading: authLoading, logout } = useAuth();
  const [models, setModels] = useState<InstalledModel[]>([]);
  const [disk, setDisk] = useState<DiskUsage | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [providerErrors, setProviderErrors] = useState<ProviderErrors>({});
  const latest = useRef(0);

  const fetchModels = useCallback(async () => {
    // Wait for auth to finish loading before fetching
    if (authLoading || !isAuthenticated) return;

    const request = ++latest.current;
    const current = () => request === latest.current;
    setLoading(true);
    setError(null);

    try {
      const response = await modelsApi.getModels(workspaceId);
      if (!current()) return;
      setModels(response.models || []);
      setProviderErrors(response.errors ?? {});
    } catch (err) {
      if (!current()) return;
      if (err instanceof ApiError && err.status === 401) {
        logout();
      }
      setError(err instanceof Error ? err.message : 'Failed to fetch models');
    } finally {
      if (current()) {
        setLoading(false);
      }
    }

    try {
      const usage = await modelsApi.getDisk();
      if (current()) setDisk(usage);
    } catch {
      if (current()) setDisk(null);
    }
  }, [authLoading, isAuthenticated, logout, workspaceId]);

  useEffect(() => {
    fetchModels();
  }, [fetchModels]);

  const deleteModel = useCallback(async (name: string): Promise<boolean> => {
    try {
      await modelsApi.deleteModel(name);
      setModels((prev) => prev.filter((m) => m.name !== name));
      return true;
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to delete model';
      setError(message);
      return false;
    }
  }, []);

  return { models, disk, loading, error, providerErrors, refresh: fetchModels, deleteModel };
}
