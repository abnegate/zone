import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ssoApi } from '../api';
import { ssoQueryKeys } from '../queryKeys';
import { SsoRefusal } from '../SsoRefusal';
import type { CreateProviderRequest, ProviderResponse, SettingsResponse } from '../types';

export interface CreateSsoProviderState {
  create(request: CreateProviderRequest, onCreated?: (provider: ProviderResponse) => void): void;
  pending: boolean;
  refusal: SsoRefusal | null;
  error: string | null;
  reset(): void;
}

export function useCreateSsoProvider(): CreateSsoProviderState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (request: CreateProviderRequest) => ssoApi.createProvider(request),
    onSuccess: (provider) => {
      queryClient.setQueryData<SettingsResponse>(ssoQueryKeys.settings, (settings) =>
        settings ? { ...settings, providers: [...settings.providers, provider] } : settings
      );
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: ssoQueryKeys.settings }),
  });

  return {
    create: (request, onCreated) =>
      mutate(request, { onSuccess: (provider) => onCreated?.(provider) }),
    pending: isPending,
    refusal: error instanceof SsoRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
