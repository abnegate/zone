import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { retryLoad } from '../../retry';
import { scimApi } from '../api';
import { scimQueryKeys } from '../queryKeys';
import type { ScimSettings } from '../types';

export interface ScimState {
  settings: ScimSettings | null;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

export function useScim(organizationId: string | null): ScimState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: scimQueryKeys.organization(organizationId ?? ''),
    queryFn: () => scimApi.settings(organizationId ?? ''),
    enabled: organizationId !== null,
    retry: retryLoad,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    settings: data ?? null,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
  };
}
