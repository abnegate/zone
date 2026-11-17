import { skipToken, useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { auditApi } from '../api';
import { auditQueryKeys } from '../queryKeys';
import type { AuditScope, RetentionResponse } from '../types';
import { retryLoad } from './retry';

export interface AuditRetentionState {
  retention: RetentionResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

export function useAuditRetention(scope: AuditScope | null): AuditRetentionState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: scope ? auditQueryKeys.retention(scope) : auditQueryKeys.idle,
    queryFn: scope ? () => auditApi.retention(scope) : skipToken,
    retry: retryLoad,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    retention: data,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
  };
}
