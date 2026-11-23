import { useMutation, useQueryClient } from '@tanstack/react-query';
import { scimApi } from '../api';
import { ScimRefusal } from '../ScimRefusal';
import type { GroupResponse, MappingRequest } from '../types';
import { cacheGroup, refreshScim } from './cache';

interface Mapping {
  groupId: string;
  request: MappingRequest;
}

export interface MapScimGroupState {
  map(groupId: string, request: MappingRequest, onMapped?: (group: GroupResponse) => void): void;
  pending: boolean;
  refusal: ScimRefusal | null;
  error: string | null;
  reset(): void;
}

export function useMapScimGroup(organizationId: string): MapScimGroupState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: ({ groupId, request }: Mapping) =>
      scimApi.mapGroup(organizationId, groupId, request),
    onSuccess: (saved) => cacheGroup(queryClient, organizationId, saved),
    onSettled: () => refreshScim(queryClient, organizationId),
  });

  return {
    map: (groupId, request, onMapped) =>
      mutate({ groupId, request }, { onSuccess: (group) => onMapped?.(group) }),
    pending: isPending,
    refusal: error instanceof ScimRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
