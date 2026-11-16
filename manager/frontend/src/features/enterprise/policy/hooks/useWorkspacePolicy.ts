import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { policyApi } from '../api';
import { policyQueryKeys } from '../queryKeys';
import type { WorkspacePolicyResponse } from '../types';
import { retryLoad } from './retry';

export interface WorkspacePolicyState {
  policy: WorkspacePolicyResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

export function useWorkspacePolicy(
  organizationId: string | null,
  workspaceId: string | null
): WorkspacePolicyState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: policyQueryKeys.workspace(organizationId ?? '', workspaceId ?? ''),
    queryFn: () => policyApi.workspace(organizationId ?? '', workspaceId ?? ''),
    enabled: organizationId !== null && workspaceId !== null,
    retry: retryLoad,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    policy: data,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
  };
}
