import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { rolesApi } from '../api';
import { rolesQueryKeys } from '../queryKeys';
import type { RolesResponse } from '../types';
import { retryLoad } from './retry';

export interface RolesState {
  response: RolesResponse | null;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

export function useRoles(organizationId: string | null): RolesState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: rolesQueryKeys.organization(organizationId ?? ''),
    queryFn: () => rolesApi.list(organizationId ?? ''),
    enabled: organizationId !== null,
    retry: retryLoad,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    response: data ?? null,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
  };
}
