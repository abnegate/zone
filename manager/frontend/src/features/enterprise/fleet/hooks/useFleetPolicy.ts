import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { retryLoad } from '../../retry';
import { fleetApi } from '../api';
import { fleetQueryKeys } from '../queryKeys';
import type { FleetPolicyResponse } from '../types';

export interface FleetPolicyState {
  policy: FleetPolicyResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

export function useFleetPolicy(): FleetPolicyState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: fleetQueryKeys.policy,
    queryFn: () => fleetApi.policy(),
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
