import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { retryLoad } from '../../retry';
import { policyApi } from '../api';
import { policyQueryKeys } from '../queryKeys';
import type { OrganizationPolicyResponse } from '../types';

export interface OrganizationPolicyState {
  policy: OrganizationPolicyResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

export function useOrganizationPolicy(organizationId: string | null): OrganizationPolicyState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: policyQueryKeys.organization(organizationId ?? ''),
    queryFn: () => policyApi.organization(organizationId ?? ''),
    enabled: organizationId !== null,
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
