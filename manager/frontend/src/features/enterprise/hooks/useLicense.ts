import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import { licenseApi } from '../api';
import { licenseQueryKeys } from '../queryKeys';
import type { LicenseResponse } from '../types';

export const LICENSE_RETRY_LIMIT = 3;

export interface LicenseState {
  response: LicenseResponse | undefined;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
}

function refused(error: Error): boolean {
  return (
    error instanceof ApiError &&
    error.status >= HttpStatus.BadRequest &&
    error.status < HttpStatus.InternalServerError
  );
}

function retryLoad(failures: number, error: Error): boolean {
  return !refused(error) && failures < LICENSE_RETRY_LIMIT;
}

export function useLicense(): LicenseState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: licenseQueryKeys.current,
    queryFn: () => licenseApi.get(),
    retry: retryLoad,
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
