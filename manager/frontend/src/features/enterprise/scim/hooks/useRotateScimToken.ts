import { useMutation, useQueryClient } from '@tanstack/react-query';
import { scimApi } from '../api';
import { ScimRefusal } from '../ScimRefusal';
import type { IssuedResponse } from '../types';
import { cacheTokens, refreshScim } from './cache';

export interface RotateScimTokenState {
  rotate(tokenId: string, onIssued: (issued: IssuedResponse) => void): void;
  pending: boolean;
  refusal: ScimRefusal | null;
  error: string | null;
  reset(): void;
}

export function useRotateScimToken(organizationId: string): RotateScimTokenState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (tokenId: string) => scimApi.rotateToken(organizationId, tokenId),
    onSuccess: ({ token }) =>
      cacheTokens(queryClient, organizationId, (tokens) =>
        tokens.map((listed) => (listed.id === token.id ? token : listed))
      ),
    onSettled: () => refreshScim(queryClient, organizationId),
    gcTime: 0,
  });

  return {
    rotate: (tokenId, onIssued) =>
      mutate(tokenId, {
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
