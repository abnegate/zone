import { skipToken, useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { auditApi } from '../api';
import { auditQueryKeys } from '../queryKeys';
import type { AuditScope, DestinationsResponse } from '../types';
import { retryLoad } from './retry';

export const DELIVERY_REFRESH_INTERVAL = 30_000;

export interface AuditDestinationsState {
  destinations: DestinationsResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

function refreshInterval(response: DestinationsResponse | undefined): number | false {
  return response?.destinations.some((destination) => destination.enabled)
    ? DELIVERY_REFRESH_INTERVAL
    : false;
}

export function useAuditDestinations(scope: AuditScope | null): AuditDestinationsState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: scope ? auditQueryKeys.destinations(scope) : auditQueryKeys.idle,
    queryFn: scope ? () => auditApi.destinations(scope) : skipToken,
    retry: retryLoad,
    refetchInterval: (query) => refreshInterval(query.state.data),
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    destinations: data,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
  };
}
