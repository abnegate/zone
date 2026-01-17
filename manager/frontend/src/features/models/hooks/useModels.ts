import { useCallback, useEffect, useState } from 'react';
import { modelsApi } from '../../../api/models';
import { useAuth } from '../../../features/auth';
import type { InstalledModel } from '../types';

export function useModels() {
  const { isAuthenticated, isLoading: authLoading, logout } = useAuth();
  const [models, setModels] = useState<InstalledModel[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchModels = useCallback(async () => {
    // Wait for auth to finish loading before fetching
    if (authLoading || !isAuthenticated) return;

    setLoading(true);
    setError(null);

    try {
      const response = await modelsApi.getModels();
      setModels(response.models || []);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to fetch models';
      if (message.includes('401')) {
        logout();
      }
      setError(message);
    } finally {
      setLoading(false);
    }
  }, [authLoading, isAuthenticated, logout]);

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

  return { models, loading, error, refresh: fetchModels, deleteModel };
}
