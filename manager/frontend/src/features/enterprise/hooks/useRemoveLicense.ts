import { useMutation, useQueryClient } from '@tanstack/react-query';
import { licenseApi } from '../api';
import { licenseMutationKeys, licenseQueryKeys } from '../queryKeys';
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
    mutationKey: licenseMutationKeys.change,
    mutationFn: () => licenseApi.remove(),
    onSuccess: async (response) => {
      await queryClient.cancelQueries({ queryKey: licenseQueryKeys.current });
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
