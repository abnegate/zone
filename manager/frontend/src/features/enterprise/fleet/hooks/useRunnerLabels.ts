import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { retryLoad } from '../../retry';
import { fleetApi } from '../api';
import type { Placement, PlacementScope } from '../placement';
import { fleetQueryKeys } from '../queryKeys';

export interface RunnerLabelsState {
  placement: Placement | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

export function useRunnerLabels(scope: PlacementScope): RunnerLabelsState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: fleetQueryKeys.placement(scope),
    queryFn: () => fleetApi.labels(scope),
    retry: retryLoad,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    placement: data,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
  };
}
