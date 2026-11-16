import { useMutation, useQueryClient } from '@tanstack/react-query';
import { licenseApi } from '../api';
import { licenseQueryKeys } from '../queryKeys';
import type { LicenseResponse } from '../types';

export interface RemoveLicenseState {
  remove(onRemoved?: (response: LicenseResponse) => void): void;
  pending: boolean;
  error: string | null;
  reset(): void;
}

export function useRemoveLicense(): RemoveLicenseState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: () => licenseApi.remove(),
    onSuccess: (response) => {
      queryClient.setQueryData(licenseQueryKeys.current, response);
    },
  });

  return {
    remove: (onRemoved) => mutate(undefined, { onSuccess: onRemoved }),
    pending: isPending,
    error: error ? error.message : null,
    reset,
  };
}
