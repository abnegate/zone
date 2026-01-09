/**
 * useSource Hook
 * Hook for managing a single source.
 */

import { useState, useEffect, useCallback } from 'react';
import { sourcesApi } from '../../../api/sources';
import type { Source, UpdateSourceRequest } from '../types';
import type { SourceVerifyResponse } from '../schemas';

export interface UseSourceResult {
  source: Source | null;
  loading: boolean;
  error: string | null;
  updateSource: (request: UpdateSourceRequest) => Promise<Source>;
  verifySource: () => Promise<SourceVerifyResponse>;
  refresh: () => Promise<void>;
}

export function useSource(id: string | null): UseSourceResult {
  const [source, setSource] = useState<Source | null>(null);
  const [loading, setLoading] = useState(!!id);
  const [error, setError] = useState<string | null>(null);

  const loadSource = useCallback(async () => {
    if (!id) {
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const data = await sourcesApi.getSource(id);
      setSource(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load source');
      setSource(null);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    loadSource();
  }, [loadSource]);

  const updateSource = useCallback(
    async (request: UpdateSourceRequest): Promise<Source> => {
      if (!id) throw new Error('No source ID provided');
      const updatedSource = await sourcesApi.updateSource(id, request);
      setSource(updatedSource);
      return updatedSource;
    },
    [id]
  );

  const verifySource = useCallback(async (): Promise<SourceVerifyResponse> => {
    if (!id) throw new Error('No source ID provided');
    const result = await sourcesApi.verifySource(id);
    // Refresh the source to get updated verification status
    const updatedSource = await sourcesApi.getSource(id);
    setSource(updatedSource);
    return result;
  }, [id]);

  const refresh = useCallback(async () => {
    await loadSource();
  }, [loadSource]);

  return {
    source,
    loading,
    error,
    updateSource,
    verifySource,
    refresh,
  };
}
