import { useMutation, useQueryClient } from '@tanstack/react-query';
import { scimApi } from '../api';
import { ScimRefusal } from '../ScimRefusal';
import { cacheTokens, refreshScim } from './cache';

export interface RevokeScimTokenState {
  revoke(tokenId: string, onRevoked?: () => void): void;
  pending: boolean;
  refusal: ScimRefusal | null;
  error: string | null;
  reset(): void;
}

export function useRevokeScimToken(organizationId: string): RevokeScimTokenState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (tokenId: string) => scimApi.revokeToken(organizationId, tokenId),
    onSuccess: (_, tokenId) =>
      cacheTokens(queryClient, organizationId, (tokens) =>
        tokens.filter((token) => token.id !== tokenId)
      ),
    onSettled: () => refreshScim(queryClient, organizationId),
  });

  return {
    revoke: (tokenId, onRevoked) => mutate(tokenId, { onSuccess: () => onRevoked?.() }),
    pending: isPending,
    refusal: error instanceof ScimRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
