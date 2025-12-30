import { useCallback, useRef, useState } from 'react';
import { client } from '../api/client';
import { useAuth } from '../context/AuthContext';
import type { BrowseModel, HuggingFaceModel, ModelSource } from '../types';

const LIMIT = 20;

export function useBrowse() {
  const { apiKey } = useAuth();
  const [source, setSource] = useState<ModelSource>('ollama');
  const [query, setQuery] = useState('');
  const [models, setModels] = useState<(BrowseModel | HuggingFaceModel)[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const offsetRef = useRef(0);

  const search = useCallback(
    async (searchQuery: string = query, searchSource: ModelSource = source) => {
      if (!apiKey) return;

      setLoading(true);
      setError(null);
      offsetRef.current = 0;

      try {
        client.setApiKey(apiKey);
        const response = await client.browseModels(searchSource, searchQuery, 0, LIMIT);
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
    [apiKey, query, source]
  );

  const loadMore = useCallback(async () => {
    if (!apiKey || loadingMore || !hasMore) return;

    setLoadingMore(true);

    try {
      client.setApiKey(apiKey);
      const response = await client.browseModels(source, query, offsetRef.current, LIMIT);
      setModels((prev) => [...prev, ...response.models]);
      setHasMore(response.has_more);
      offsetRef.current += response.models.length;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load more models');
    } finally {
      setLoadingMore(false);
    }
  }, [apiKey, source, query, loadingMore, hasMore]);

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
