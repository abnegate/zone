import { useCallback, useEffect, useState } from 'react';
import { client } from '../api/client';
import { useAuth } from '../context/AuthContext';
import type { InstalledModel } from '../types';

export function useModels() {
  const { apiKey, logout } = useAuth();
  const [models, setModels] = useState<InstalledModel[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchModels = useCallback(async () => {
    if (!apiKey) return;

    setLoading(true);
    setError(null);

    try {
      client.setApiKey(apiKey);
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
  }, [apiKey, logout]);

  useEffect(() => {
    fetchModels();
  }, [fetchModels]);

  const deleteModel = useCallback(
    async (name: string): Promise<boolean> => {
      try {
        client.setApiKey(apiKey);
        await client.deleteModel(name);
        setModels((prev) => prev.filter((m) => m.name !== name));
        return true;
      } catch (err) {
        const message = err instanceof Error ? err.message : 'Failed to delete model';
        setError(message);
        return false;
      }
    },
    [apiKey]
  );

  return { models, loading, error, refresh: fetchModels, deleteModel };
}
