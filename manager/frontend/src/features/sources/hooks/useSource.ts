/**
 * useSource Hook
 * Hook for managing a single source.
 */

import { useCallback, useEffect, useState } from 'react';
import { sourcesApi } from '../../../api/sources';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { SourceVerifyResponse } from '../schemas';
import type { Source, UpdateSourceRequest } from '../types';

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
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const loadSource = useCallback(async () => {
    if (!id || !workspaceId) {
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const data = await sourcesApi.getSource(workspaceId, id);
      setSource(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load source');
      setSource(null);
    } finally {
      setLoading(false);
    }
  }, [id, workspaceId]);

  useEffect(() => {
    loadSource();
  }, [loadSource]);

  const updateSource = useCallback(
    async (request: UpdateSourceRequest): Promise<Source> => {
      if (!id) throw new Error('No source ID provided');
      if (!workspaceId) throw new Error('No workspace selected');
      const updatedSource = await sourcesApi.updateSource(workspaceId, id, request);
      setSource(updatedSource);
      return updatedSource;
    },
    [id, workspaceId]
  );

  const verifySource = useCallback(async (): Promise<SourceVerifyResponse> => {
    if (!id) throw new Error('No source ID provided');
    if (!workspaceId) throw new Error('No workspace selected');
    const result = await sourcesApi.verifySource(workspaceId, id);
    // Refresh the source to get updated verification status
    const updatedSource = await sourcesApi.getSource(workspaceId, id);
    setSource(updatedSource);
    return result;
  }, [id, workspaceId]);

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
