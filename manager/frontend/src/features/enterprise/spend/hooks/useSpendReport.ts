import { partialMatchKey, useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { retryLoad } from '../../retry';
import { spendApi } from '../api';
import { spendQueryKeys } from '../queryKeys';
import type { SpendQuery, SpendReportResponse } from '../schemas';

export interface SpendReportState {
  response: SpendReportResponse | undefined;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

export function useSpendReport(organizationId: string, query: SpendQuery): SpendReportState {
  const { data, isPending, isFetching, isPlaceholderData, error, refetch } = useQuery({
    queryKey: spendQueryKeys.report(organizationId, query),
    queryFn: () => spendApi.report(organizationId, query),
    retry: retryLoad,
    placeholderData: (previous, previousQuery) =>
      previousQuery &&
      partialMatchKey(previousQuery.queryKey, spendQueryKeys.reports(organizationId))
        ? previous
        : undefined,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    response: data,
    loading: isPending,
    refreshing: isFetching && isPlaceholderData,
    error: error ? error.message : null,
    refresh,
  };
}
