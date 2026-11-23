import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { retryLoad } from '../../retry';
import { fleetApi } from '../api';
import { fleetQueryKeys } from '../queryKeys';
import type { RunnersResponse } from '../types';

export const RUNNERS_REFRESH_INTERVAL = 15_000;

export interface RunnersState {
  runners: RunnersResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

export function useRunners(): RunnersState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: fleetQueryKeys.runners,
    queryFn: () => fleetApi.runners(),
    retry: retryLoad,
    refetchInterval: RUNNERS_REFRESH_INTERVAL,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    runners: data,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
  };
}
