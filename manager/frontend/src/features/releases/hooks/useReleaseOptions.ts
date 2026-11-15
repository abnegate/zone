import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { releasesApi } from '../../../api/releases';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { errorMessage, NO_WORKSPACE, retryRead } from '../../github';
import type { OptionsResponse, SourceOptions } from '../types';
import { releaseOptionsKey } from './keys';

export const OPTIONS_FAILED = 'Failed to load release options';

const NO_SOURCES: readonly SourceOptions[] = [];

export interface ReleaseOptions {
  options: OptionsResponse | undefined;
  sources: readonly SourceOptions[];
  loading: boolean;
  error: string | null;
  reload: () => Promise<void>;
  reloading: boolean;
}

export function useReleaseOptions(open: boolean): ReleaseOptions {
  const queryClient = useQueryClient();
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const {
    data: options,
    isLoading,
    error,
  } = useQuery({
    queryKey: releaseOptionsKey(workspaceId),
    queryFn: ({ signal }) => {
      if (!workspaceId) throw new Error(NO_WORKSPACE);
      return releasesApi.options(workspaceId, {}, signal);
    },
    enabled: open && !!workspaceId,
    retry: retryRead,
  });

  const { mutateAsync, isPending: reloading } = useMutation({
    mutationFn: (target: string) => releasesApi.options(target, { refresh: true }),
    onSuccess: async (reloaded, target) => {
      const queryKey = releaseOptionsKey(target);
      await queryClient.cancelQueries({ queryKey });
      queryClient.setQueryData(queryKey, reloaded);
    },
  });

  const reload = useCallback(async () => {
    if (!workspaceId) throw new Error(NO_WORKSPACE);
    await mutateAsync(workspaceId);
  }, [mutateAsync, workspaceId]);

  return {
    options,
    sources: options?.sources ?? NO_SOURCES,
    loading: open && isLoading,
    error: errorMessage(error, OPTIONS_FAILED),
    reload,
    reloading,
  };
}
