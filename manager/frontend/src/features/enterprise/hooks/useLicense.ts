import { type Query, useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { licenseApi } from '../api';
import { licenseQueryKeys } from '../queryKeys';
import { refused, retryLoad } from '../retry';
import type { LicenseResponse } from '../types';

export interface LicenseState {
  response: LicenseResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

function retryMount(query: Query<LicenseResponse>): boolean {
  const { error } = query.state;
  return error === null || !refused(error);
}

export interface LicenseOptions {
  enabled?: boolean;
}

export function useLicense({ enabled = true }: LicenseOptions = {}): LicenseState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: licenseQueryKeys.current,
    queryFn: () => licenseApi.get(),
    retry: retryLoad,
    retryOnMount: retryMount,
    enabled,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    response: data,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
  };
}
