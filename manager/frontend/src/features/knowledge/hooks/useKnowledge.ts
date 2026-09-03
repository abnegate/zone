import { useCallback, useEffect, useState } from 'react';
import { knowledgeApi } from '../../../api/knowledge';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { KnowledgeEntry } from '../types';

export function useKnowledge() {
  const [entries, setEntries] = useState<KnowledgeEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState<string | null>(null);
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const loadEntries = useCallback(async () => {
    if (!workspaceId) {
      setEntries([]);
      setLoading(false);
      return;
    }
    try {
      setLoading(true);
      setError(null);
      const response = await knowledgeApi.getKnowledge(workspaceId);
      setEntries(response.entries);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to load knowledge';
      setError(message.startsWith('Validation failed') ? 'Couldn’t load knowledge' : message);
    } finally {
      setLoading(false);
    }
  }, [workspaceId]);

  useEffect(() => {
    loadEntries();
  }, [loadEntries]);

  const createEntry = useCallback(
    async (
      request: Omit<import('../types').CreateKnowledgeRequest, 'workspace_id'>
    ): Promise<KnowledgeEntry> => {
      if (!workspaceId) throw new Error('No workspace selected');
      const newEntry = await knowledgeApi.createKnowledge({
        ...request,
        workspace_id: workspaceId,
      });
      setEntries((prev) => [newEntry, ...prev]);
      return newEntry;
    },
    [workspaceId]
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
