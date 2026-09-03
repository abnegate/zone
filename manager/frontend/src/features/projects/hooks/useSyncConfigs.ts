import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { projectsApi } from '../../../api/projects';
import type { CreateSyncConfigRequest } from '../types';

export function useSyncConfigs(projectId: string | null) {
  const queryClient = useQueryClient();
  const queryKey = ['syncConfigs', projectId];

  const {
    data: configs = [],
    isLoading: loading,
    error,
    refetch,
  } = useQuery({
    queryKey,
    queryFn: () => {
      if (!projectId) return [];
      return projectsApi.getSyncConfigs(projectId);
    },
    enabled: !!projectId,
  });

  const createSyncConfigMutation = useMutation({
    mutationFn: (request: CreateSyncConfigRequest) => {
      if (!projectId) throw new Error('Project ID is required');
      return projectsApi.createSyncConfig(projectId, request);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
    },
  });

  const deleteSyncConfigMutation = useMutation({
    mutationFn: (configId: string) => {
      if (!projectId) throw new Error('Project ID is required');
      return projectsApi.deleteSyncConfig(projectId, configId);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
    },
  });

  return {
    configs,
    loading,
    error: error instanceof Error ? error.message : error ? 'Failed to load sync configs' : null,
    createSyncConfig: createSyncConfigMutation.mutateAsync,
    deleteSyncConfig: deleteSyncConfigMutation.mutateAsync,
    refetch,
  };
}
