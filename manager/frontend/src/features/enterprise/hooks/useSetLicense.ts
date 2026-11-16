import { useMutation, useQueryClient } from '@tanstack/react-query';
import { licenseApi } from '../api';
import { LicenseRefusal } from '../LicenseRefusal';
import { licenseMutationKeys, licenseQueryKeys } from '../queryKeys';
import type { LicenseResponse } from '../types';

export interface SetLicenseState {
  set(key: string, onSaved?: (response: LicenseResponse) => void): void;
  pending: boolean;
  refusal: LicenseRefusal | null;
  error: string | null;
  reset(): void;
}

export function useSetLicense(): SetLicenseState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationKey: licenseMutationKeys.change,
    mutationFn: (key: string) => licenseApi.set(key),
    onSuccess: async (response) => {
      await queryClient.cancelQueries({ queryKey: licenseQueryKeys.current });
      queryClient.setQueryData(licenseQueryKeys.current, response);
    },
  });

  return {
    set: (key, onSaved) => mutate(key, { onSuccess: onSaved }),
    pending: isPending,
    refusal: error instanceof LicenseRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
