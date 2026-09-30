import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { projectsApi } from '../../../api/projects';
import type { CreateSyncConfigRequest, SyncConfigSecret } from '../types';

interface WebhookSecretChange {
  configId: string;
  secret?: string;
}

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
    gcTime: 0,
  });

  const setWebhookSecretMutation = useMutation({
    mutationFn: ({ configId, secret }: WebhookSecretChange) => {
      if (!projectId) throw new Error('Project ID is required');
      return projectsApi.setWebhookSecret(projectId, configId, secret);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
    },
    gcTime: 0,
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

  const createSyncConfig = async (request: CreateSyncConfigRequest): Promise<SyncConfigSecret> => {
    try {
      return await createSyncConfigMutation.mutateAsync(request);
    } finally {
      createSyncConfigMutation.reset();
    }
  };

  const setWebhookSecret = async (configId: string, secret?: string): Promise<SyncConfigSecret> => {
    try {
      return await setWebhookSecretMutation.mutateAsync({ configId, secret });
    } finally {
      setWebhookSecretMutation.reset();
    }
  };

  return {
    configs,
    loading,
    error: error instanceof Error ? error.message : error ? 'Failed to load sync configs' : null,
    createSyncConfig,
    setWebhookSecret,
    deleteSyncConfig: deleteSyncConfigMutation.mutateAsync,
    refetch,
  };
}
