/**
 * useSources Hook
 * Hook for managing a list of sources with CRUD operations.
 */

import { useCallback, useEffect, useState } from 'react';
import { sourcesApi } from '../../../api/sources';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { SourceVerifyResponse } from '../schemas';
import type { CreateSourceRequest, Source, SourceType, UpdateSourceRequest } from '../types';

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
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const { type, activeOnly = false } = options;

  const loadSources = useCallback(async () => {
    if (!workspaceId) {
      setSources([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const data = await sourcesApi.getSources(workspaceId, type, activeOnly);
      setSources(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load sources');
      setSources([]);
    } finally {
      setLoading(false);
    }
  }, [workspaceId, type, activeOnly]);

  useEffect(() => {
    loadSources();
  }, [loadSources]);

  const createSource = useCallback(
    async (request: CreateSourceRequest): Promise<Source> => {
      if (!workspaceId) throw new Error('No workspace selected');
      const newSource = await sourcesApi.createSource(workspaceId, request);
      setSources((prev) => [newSource, ...prev]);
      return newSource;
    },
    [workspaceId]
  );

  const updateSource = useCallback(
    async (id: string, request: UpdateSourceRequest): Promise<Source> => {
      if (!workspaceId) throw new Error('No workspace selected');
      const updatedSource = await sourcesApi.updateSource(workspaceId, id, request);
      setSources((prev) => prev.map((s) => (s.id === id ? updatedSource : s)));
      return updatedSource;
    },
    [workspaceId]
  );

  const deleteSource = useCallback(
    async (id: string): Promise<void> => {
      if (!workspaceId) throw new Error('No workspace selected');
      await sourcesApi.deleteSource(workspaceId, id);
      setSources((prev) => prev.filter((s) => s.id !== id));
    },
    [workspaceId]
  );

  const verifySource = useCallback(
    async (id: string): Promise<SourceVerifyResponse> => {
      if (!workspaceId) throw new Error('No workspace selected');
      const result = await sourcesApi.verifySource(workspaceId, id);
      // Refresh the source to get updated verification status
      const updatedSource = await sourcesApi.getSource(workspaceId, id);
      setSources((prev) => prev.map((s) => (s.id === id ? updatedSource : s)));
      return result;
    },
    [workspaceId]
  );

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
