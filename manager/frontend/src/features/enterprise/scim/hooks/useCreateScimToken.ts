import { useMutation, useQueryClient } from '@tanstack/react-query';
import { scimApi } from '../api';
import { ScimRefusal } from '../ScimRefusal';
import type { IssuedResponse } from '../types';
import { cacheTokens, refreshScim } from './cache';

export interface CreateScimTokenState {
  create(name: string, onIssued: (issued: IssuedResponse) => void): void;
  pending: boolean;
  refusal: ScimRefusal | null;
  error: string | null;
  reset(): void;
}

export function useCreateScimToken(organizationId: string): CreateScimTokenState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (name: string) => scimApi.createToken(organizationId, name),
    onSuccess: ({ token }) =>
      cacheTokens(queryClient, organizationId, (tokens) => [...tokens, token]),
    onSettled: () => refreshScim(queryClient, organizationId),
    gcTime: 0,
  });

  return {
    create: (name, onIssued) =>
      mutate(name, {
        onSuccess: (issued) => {
          reset();
          onIssued(issued);
        },
      }),
    pending: isPending,
    refusal: error instanceof ScimRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
