import { useCallback, useState } from 'react';
import { knowledgeApi } from '../../../api/knowledge';
import type { SearchOptions, SearchResult } from '../types';

export function useContextSearch() {
  const [results, setResults] = useState<SearchResult[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const search = useCallback(async (options: SearchOptions) => {
    try {
      setLoading(true);
      setError(null);
      const response = await knowledgeApi.searchContext(options);
      setResults(response.results);
      setTotal(response.total);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to search');
      setResults([]);
      setTotal(0);
    } finally {
      setLoading(false);
    }
  }, []);

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
