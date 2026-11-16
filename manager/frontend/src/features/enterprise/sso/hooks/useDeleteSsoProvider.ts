import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ssoApi } from '../api';
import { ssoQueryKeys } from '../queryKeys';
import { SsoRefusal } from '../SsoRefusal';
import type { SettingsResponse } from '../types';

export interface DeleteSsoProviderState {
  remove(providerId: string, onDeleted?: () => void): void;
  pending: boolean;
  refusal: SsoRefusal | null;
  error: string | null;
  reset(): void;
}

export function useDeleteSsoProvider(): DeleteSsoProviderState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (providerId: string) => ssoApi.deleteProvider(providerId),
    onSuccess: (_, providerId) => {
      queryClient.setQueryData<SettingsResponse>(ssoQueryKeys.settings, (settings) =>
        settings
          ? {
              ...settings,
              providers: settings.providers.filter((provider) => provider.id !== providerId),
            }
          : settings
      );
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: ssoQueryKeys.settings }),
  });

  return {
    remove: (providerId, onDeleted) => mutate(providerId, { onSuccess: () => onDeleted?.() }),
    pending: isPending,
    refusal: error instanceof SsoRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
