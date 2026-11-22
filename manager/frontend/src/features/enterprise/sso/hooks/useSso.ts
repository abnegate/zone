import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { retryLoad } from '../../retry';
import { ssoApi } from '../api';
import { ssoQueryKeys } from '../queryKeys';
import type { SettingsResponse } from '../types';

export interface SsoState {
  settings: SettingsResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

export function useSso(): SsoState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ssoQueryKeys.settings,
    queryFn: () => ssoApi.get(),
    retry: retryLoad,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    settings: data,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
  };
}
