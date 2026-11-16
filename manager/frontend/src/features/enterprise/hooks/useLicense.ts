import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { licenseApi } from '../api';
import { licenseQueryKeys } from '../queryKeys';
import type { LicenseResponse } from '../types';

export interface LicenseState {
  response: LicenseResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

export function useLicense(): LicenseState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: licenseQueryKeys.current,
    queryFn: () => licenseApi.get(),
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
