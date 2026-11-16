import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ssoApi } from '../api';
import { ssoQueryKeys } from '../queryKeys';
import { SsoRefusal } from '../SsoRefusal';
import type { ProviderResponse, SettingsResponse, UpdateProviderRequest } from '../types';

interface ProviderUpdate {
  providerId: string;
  request: UpdateProviderRequest;
}

export interface UpdateSsoProviderState {
  update(
    providerId: string,
    request: UpdateProviderRequest,
    onUpdated?: (provider: ProviderResponse) => void
  ): void;
  pending: boolean;
  refusal: SsoRefusal | null;
  error: string | null;
  reset(): void;
}

export function useUpdateSsoProvider(): UpdateSsoProviderState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: ({ providerId, request }: ProviderUpdate) =>
      ssoApi.updateProvider(providerId, request),
    onSuccess: (provider) => {
      queryClient.setQueryData<SettingsResponse>(ssoQueryKeys.settings, (settings) =>
        settings
          ? {
              ...settings,
              providers: settings.providers.map((current) =>
                current.id === provider.id ? provider : current
              ),
            }
          : settings
      );
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: ssoQueryKeys.settings }),
  });

  return {
    update: (providerId, request, onUpdated) =>
      mutate({ providerId, request }, { onSuccess: (provider) => onUpdated?.(provider) }),
    pending: isPending,
    refusal: error instanceof SsoRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
