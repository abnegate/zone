import { useCallback, useRef, useState } from 'react';
import { modelsApi } from '../../../api/models';
import { useAuth } from '../../../features/auth';
import type { BrowseModel, ModelSource } from '../types';

const LIMIT = 20;

export function useBrowse() {
  const { isAuthenticated } = useAuth();
  const [source, setSource] = useState<ModelSource>('ollama');
  const [query, setQuery] = useState('');
  const [models, setModels] = useState<BrowseModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const offsetRef = useRef(0);

  const search = useCallback(
    async (searchQuery: string = query, searchSource: ModelSource = source) => {
      if (!isAuthenticated) return;

      setLoading(true);
      setError(null);
      offsetRef.current = 0;

      try {
        const response = await modelsApi.browseModels(searchSource, searchQuery, 0, LIMIT);
        setModels(response.models);
        setHasMore(response.has_more);
        offsetRef.current = response.models.length;
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to browse models');
        setModels([]);
      } finally {
        setLoading(false);
      }
    },
    [isAuthenticated, query, source]
  );

  const loadMore = useCallback(async () => {
    if (!isAuthenticated || loadingMore || !hasMore) return;

    setLoadingMore(true);

    try {
      const response = await modelsApi.browseModels(source, query, offsetRef.current, LIMIT);
      setModels((prev) => [...prev, ...response.models]);
      setHasMore(response.has_more);
      offsetRef.current += response.models.length;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load more models');
    } finally {
      setLoadingMore(false);
    }
  }, [isAuthenticated, source, query, loadingMore, hasMore]);

  const changeSource = useCallback(
    (newSource: ModelSource) => {
      setSource(newSource);
      setQuery('');
      setModels([]);
      search('', newSource);
    },
    [search]
  );

  return {
    source,
    query,
    setQuery,
    models,
    loading,
    loadingMore,
    hasMore,
    error,
    search,
    loadMore,
    changeSource,
  };
}
