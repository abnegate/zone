import { useCallback, useState } from 'react';
import { knowledgeApi } from '../../../api/knowledge';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { SearchResult } from '../types';

export interface ContextSearchOptions {
  query: string;
  mode?: 'hybrid' | 'semantic' | 'keyword';
  source_ids?: string[];
  limit?: number;
}

export function useContextSearch() {
  const [results, setResults] = useState<SearchResult[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const search = useCallback(
    async (options: ContextSearchOptions) => {
      if (!workspaceId) {
        setError('No workspace selected');
        return;
      }
      try {
        setLoading(true);
        setError(null);
        const response = await knowledgeApi.searchContext({
          ...options,
          workspace_id: workspaceId,
        });
        setResults(response.results);
        setTotal(response.total);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to search');
        setResults([]);
        setTotal(0);
      } finally {
        setLoading(false);
      }
    },
    [workspaceId]
  );

  const clear = useCallback(() => {
    setResults([]);
    setTotal(0);
    setError(null);
  }, []);

  return {
    results,
    total,
    loading,
    error,
    search,
    clear,
  };
}
