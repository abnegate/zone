/**
 * useSources Hook
 * Hook for managing a list of sources with CRUD operations.
 */

import { useState, useEffect, useCallback } from 'react';
import { sourcesApi } from '../../../api/sources';
import type { Source, SourceType, CreateSourceRequest, UpdateSourceRequest } from '../types';
import type { SourceVerifyResponse } from '../schemas';

export interface UseSourcesOptions {
  type?: SourceType;
  activeOnly?: boolean;
}

export interface UseSourcesResult {
  sources: Source[];
  loading: boolean;
  error: string | null;
  createSource: (request: CreateSourceRequest) => Promise<Source>;
  updateSource: (id: string, request: UpdateSourceRequest) => Promise<Source>;
  deleteSource: (id: string) => Promise<void>;
  verifySource: (id: string) => Promise<SourceVerifyResponse>;
  refresh: () => Promise<void>;
}

export function useSources(options: UseSourcesOptions = {}): UseSourcesResult {
  const [sources, setSources] = useState<Source[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const { type, activeOnly = false } = options;

  const loadSources = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await sourcesApi.getSources(type, activeOnly);
      setSources(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load sources');
      setSources([]);
    } finally {
      setLoading(false);
    }
  }, [type, activeOnly]);

  useEffect(() => {
    loadSources();
  }, [loadSources]);

  const createSource = useCallback(async (request: CreateSourceRequest): Promise<Source> => {
    const newSource = await sourcesApi.createSource(request);
    setSources((prev) => [newSource, ...prev]);
    return newSource;
  }, []);

  const updateSource = useCallback(
    async (id: string, request: UpdateSourceRequest): Promise<Source> => {
      const updatedSource = await sourcesApi.updateSource(id, request);
      setSources((prev) => prev.map((s) => (s.id === id ? updatedSource : s)));
      return updatedSource;
    },
    []
  );

  const deleteSource = useCallback(async (id: string): Promise<void> => {
    await sourcesApi.deleteSource(id);
    setSources((prev) => prev.filter((s) => s.id !== id));
  }, []);

  const verifySource = useCallback(async (id: string): Promise<SourceVerifyResponse> => {
    const result = await sourcesApi.verifySource(id);
    // Refresh the source to get updated verification status
    const updatedSource = await sourcesApi.getSource(id);
    setSources((prev) => prev.map((s) => (s.id === id ? updatedSource : s)));
    return result;
  }, []);

  const refresh = useCallback(async () => {
    await loadSources();
  }, [loadSources]);

  return {
    sources,
    loading,
    error,
    createSource,
    updateSource,
    deleteSource,
    verifySource,
    refresh,
  };
}
