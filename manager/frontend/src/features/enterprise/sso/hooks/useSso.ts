import { useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { ApiError } from '../../../../api/ApiError';
import { HttpStatus } from '../../../../api/HttpStatus';
import { ssoApi } from '../api';
import { ssoQueryKeys } from '../queryKeys';
import type { SettingsResponse } from '../types';

export const SSO_RETRY_LIMIT = 3;

export interface SsoState {
  settings: SettingsResponse | undefined;
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
  return !refused(error) && failures < SSO_RETRY_LIMIT;
}

export function useSso(): SsoState {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ssoQueryKeys.settings,
    queryFn: () => ssoApi.get(),
    retry: retryLoad,
  });

  const refresh = useCallback(async (): Promise<void> => {
    await refetch();
  }, [refetch]);

  return {
    settings: data,
    loading: isLoading,
    error: error ? error.message : null,
    refresh,
  };
}
