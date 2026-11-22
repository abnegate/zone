import { useMutation, useQueryClient } from '@tanstack/react-query';
import { scimApi } from '../api';
import { ScimRefusal } from '../ScimRefusal';
import type { GroupResponse } from '../types';
import { cacheGroup, refreshScim } from './cache';

export interface UnmapScimGroupState {
  unmap(groupId: string, onUnmapped?: (group: GroupResponse) => void): void;
  pending: boolean;
  refusal: ScimRefusal | null;
  error: string | null;
  reset(): void;
}

export function useUnmapScimGroup(organizationId: string): UnmapScimGroupState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (groupId: string) => scimApi.unmapGroup(organizationId, groupId),
    onSuccess: (saved) => cacheGroup(queryClient, organizationId, saved),
    onSettled: () => refreshScim(queryClient, organizationId),
  });

  return {
    unmap: (groupId, onUnmapped) => mutate(groupId, { onSuccess: (group) => onUnmapped?.(group) }),
    pending: isPending,
    refusal: error instanceof ScimRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
