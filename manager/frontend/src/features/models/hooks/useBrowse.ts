import { useCallback, useRef, useState } from 'react';
import { modelsApi } from '../../../api/models';
import { useAuth } from '../../../features/auth';
import type { BrowseModel, BrowseSource, ModelSource } from '../types';
import { ALL_SOURCES } from '../types';

const LIMIT = 20;
const LIMIT_PER_SOURCE = 5; // For "all" mode, fetch fewer per source to interleave

interface SourceState {
  cursor: string | null;
  hasMore: boolean;
}

export function useBrowse() {
  const { isAuthenticated } = useAuth();
  const [source, setSource] = useState<BrowseSource>('all');
  const [query, setQuery] = useState('');
  const [models, setModels] = useState<BrowseModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Single source cursor
  const cursorRef = useRef<string | null>(null);

  // Multi-source cursors for "all" mode
  const sourceCursorsRef = useRef<Record<ModelSource, SourceState>>({
    ollama: { cursor: null, hasMore: true },
    huggingface: { cursor: null, hasMore: true },
    gpt4all: { cursor: null, hasMore: true },
    openrouter: { cursor: null, hasMore: true },
  });

  // Interleave results from multiple sources (round-robin)
  const interleaveResults = useCallback((resultsBySource: Record<ModelSource, BrowseModel[]>): BrowseModel[] => {
    const interleaved: BrowseModel[] = [];
    const sources = ALL_SOURCES.filter(s => resultsBySource[s]?.length > 0);

    if (sources.length === 0) return [];

    const maxLength = Math.max(...sources.map(s => resultsBySource[s].length));

    for (let i = 0; i < maxLength; i++) {
      for (const src of sources) {
        if (i < resultsBySource[src].length) {
          interleaved.push(resultsBySource[src][i]);
        }
      }
    }

    return interleaved;
  }, []);

  const searchAllSources = useCallback(
    async (searchQuery: string) => {
      // Reset all source cursors
      for (const src of ALL_SOURCES) {
        sourceCursorsRef.current[src] = { cursor: null, hasMore: true };
      }

      // Fetch from all sources in parallel
      const results = await Promise.allSettled(
        ALL_SOURCES.map(async (src) => {
          const response = await modelsApi.browseModels(src, searchQuery, null, LIMIT_PER_SOURCE);
          // Tag each model with its source
          const modelsWithSource = response.models.map(m => ({ ...m, source: src }));
          sourceCursorsRef.current[src] = {
            cursor: response.next_cursor,
            hasMore: response.next_cursor !== null,
          };
          return { source: src, models: modelsWithSource };
        })
      );

      // Collect successful results
      const resultsBySource: Record<ModelSource, BrowseModel[]> = {
        ollama: [],
        huggingface: [],
        gpt4all: [],
        openrouter: [],
      };

      for (const result of results) {
        if (result.status === 'fulfilled') {
          resultsBySource[result.value.source] = result.value.models;
        }
      }

      // Check if any source has more results
      const anyHasMore = ALL_SOURCES.some(src => sourceCursorsRef.current[src].hasMore);

      return {
        models: interleaveResults(resultsBySource),
        hasMore: anyHasMore,
      };
    },
    [interleaveResults]
  );

  const loadMoreAllSources = useCallback(
    async (searchQuery: string) => {
      // Find sources that have more results
      const sourcesWithMore = ALL_SOURCES.filter(src => sourceCursorsRef.current[src].hasMore);

      if (sourcesWithMore.length === 0) {
        return { models: [], hasMore: false };
      }

      // Fetch more from sources that have more results
      const results = await Promise.allSettled(
        sourcesWithMore.map(async (src) => {
          const state = sourceCursorsRef.current[src];
          const response = await modelsApi.browseModels(src, searchQuery, state.cursor, LIMIT_PER_SOURCE);
          const modelsWithSource = response.models.map(m => ({ ...m, source: src }));
          sourceCursorsRef.current[src] = {
            cursor: response.next_cursor,
            hasMore: response.next_cursor !== null,
          };
          return { source: src, models: modelsWithSource };
        })
      );

      // Collect successful results
      const resultsBySource: Record<ModelSource, BrowseModel[]> = {
        ollama: [],
        huggingface: [],
        gpt4all: [],
        openrouter: [],
      };

      for (const result of results) {
        if (result.status === 'fulfilled') {
          resultsBySource[result.value.source] = result.value.models;
        }
      }

      const anyHasMore = ALL_SOURCES.some(src => sourceCursorsRef.current[src].hasMore);

      return {
        models: interleaveResults(resultsBySource),
        hasMore: anyHasMore,
      };
    },
    [interleaveResults]
  );

  const search = useCallback(
    async (searchQuery: string = query, searchSource: BrowseSource = source) => {
      if (!isAuthenticated) return;

      setLoading(true);
      setError(null);
      cursorRef.current = null;

      try {
        if (searchSource === 'all') {
          const result = await searchAllSources(searchQuery);
          setModels(result.models);
          setHasMore(result.hasMore);
        } else {
          const response = await modelsApi.browseModels(searchSource, searchQuery, null, LIMIT);
          // Tag models with source for consistency
          const modelsWithSource = response.models.map(m => ({ ...m, source: searchSource }));
          setModels(modelsWithSource);
          setHasMore(response.next_cursor !== null);
          cursorRef.current = response.next_cursor;
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to browse models');
        setModels([]);
      } finally {
        setLoading(false);
      }
    },
    [isAuthenticated, query, source, searchAllSources]
  );

  const loadMore = useCallback(async () => {
    if (!isAuthenticated || loadingMore || !hasMore) return;

    setLoadingMore(true);

    try {
      if (source === 'all') {
        const result = await loadMoreAllSources(query);
        setModels((prev) => [...prev, ...result.models]);
        setHasMore(result.hasMore);
      } else {
        const response = await modelsApi.browseModels(source, query, cursorRef.current, LIMIT);
        const modelsWithSource = response.models.map(m => ({ ...m, source }));
        setModels((prev) => [...prev, ...modelsWithSource]);
        setHasMore(response.next_cursor !== null);
        cursorRef.current = response.next_cursor;
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load more models');
    } finally {
      setLoadingMore(false);
    }
  }, [isAuthenticated, source, query, loadingMore, hasMore, loadMoreAllSources]);

  const changeSource = useCallback(
    (newSource: BrowseSource) => {
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
