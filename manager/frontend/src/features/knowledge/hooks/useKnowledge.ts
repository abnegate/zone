import { useCallback, useEffect, useState } from 'react';
import { knowledgeApi } from '../../../api/knowledge';
import type { CreateKnowledgeRequest, KnowledgeEntry } from '../types';

export function useKnowledge(workspaceId?: string) {
  const [entries, setEntries] = useState<KnowledgeEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState<string | null>(null);

  const loadEntries = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const response = await knowledgeApi.getKnowledge(workspaceId);
      setEntries(response.entries);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load knowledge');
    } finally {
      setLoading(false);
    }
  }, [workspaceId]);

  useEffect(() => {
    loadEntries();
  }, [loadEntries]);

  const createEntry = useCallback(
    async (request: CreateKnowledgeRequest): Promise<KnowledgeEntry> => {
      const newEntry = await knowledgeApi.createKnowledge(request);
      setEntries((prev) => [newEntry, ...prev]);
      return newEntry;
    },
    []
  );

  const deleteEntry = useCallback(async (id: string): Promise<void> => {
    await knowledgeApi.deleteKnowledge(id);
    setEntries((prev) => prev.filter((e) => e.id !== id));
  }, []);

  const refreshEntry = useCallback(async (id: string): Promise<KnowledgeEntry> => {
    try {
      setRefreshing(id);
      const refreshedEntry = await knowledgeApi.refreshKnowledge(id);
      setEntries((prev) => prev.map((e) => (e.id === id ? refreshedEntry : e)));
      return refreshedEntry;
    } finally {
      setRefreshing(null);
    }
  }, []);

  const reload = useCallback(async () => {
    await loadEntries();
  }, [loadEntries]);

  return {
    entries,
    loading,
    error,
    refreshing,
    createEntry,
    deleteEntry,
    refreshEntry,
    reload,
  };
}
