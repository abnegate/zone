import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ssoApi } from '../api';
import { ssoQueryKeys } from '../queryKeys';
import { SsoRefusal } from '../SsoRefusal';
import type { EnforcementResponse, SettingsResponse } from '../types';

export interface SetSsoEnforcementState {
  set(required: boolean, onSet?: (response: EnforcementResponse) => void): void;
  pending: boolean;
  refusal: SsoRefusal | null;
  error: string | null;
  reset(): void;
}

export function useSetSsoEnforcement(): SetSsoEnforcementState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (required: boolean) => ssoApi.setEnforcement(required),
    onSuccess: (response) => {
      queryClient.setQueryData<SettingsResponse>(ssoQueryKeys.settings, (settings) =>
        settings ? { ...settings, required: response.required } : settings
      );
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: ssoQueryKeys.settings }),
  });

  return {
    set: (required, onSet) => mutate(required, { onSuccess: (response) => onSet?.(response) }),
    pending: isPending,
    refusal: error instanceof SsoRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
