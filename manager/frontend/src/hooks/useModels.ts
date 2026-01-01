import { useCallback, useEffect, useState } from 'react';
import { client } from '../api/client';
import { useAuth } from '../context/AuthContext';
import type { InstalledModel } from '../types';

export function useModels() {
  const { isAuthenticated, logout } = useAuth();
  const [models, setModels] = useState<InstalledModel[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchModels = useCallback(async () => {
    if (!isAuthenticated) return;

    setLoading(true);
    setError(null);

    try {
      const response = await client.getModels();
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
  }, [isAuthenticated, logout]);

  useEffect(() => {
    fetchModels();
  }, [fetchModels]);

  const deleteModel = useCallback(async (name: string): Promise<boolean> => {
    try {
      await client.deleteModel(name);
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
