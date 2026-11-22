import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { retryLoad } from '../../policy/hooks/retry';
import { spendApi } from '../api';
import { spendQueryKeys } from '../queryKeys';
import type { BudgetsResponse } from '../schemas';

export interface BudgetsState {
  response: BudgetsResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

export function useBudgets(organizationId: string): BudgetsState {
  const { data, isPending, error, refetch } = useQuery({
    queryKey: spendQueryKeys.budgets(organizationId),
    queryFn: () => spendApi.budgets(organizationId),
    retry: retryLoad,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    response: data,
    loading: isPending,
    error: error ? error.message : null,
    refresh,
  };
}
