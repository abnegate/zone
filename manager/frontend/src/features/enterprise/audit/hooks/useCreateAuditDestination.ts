import { useMutation, useQueryClient } from '@tanstack/react-query';
import { AuditRefusal } from '../AuditRefusal';
import { auditApi } from '../api';
import { auditQueryKeys } from '../queryKeys';
import type {
  AuditScope,
  CreateDestinationRequest,
  DestinationResponse,
  DestinationsResponse,
} from '../types';

export interface CreateAuditDestinationState {
  create(
    request: CreateDestinationRequest,
    onCreated?: (destination: DestinationResponse) => void
  ): void;
  pending: boolean;
  refusal: AuditRefusal | null;
  error: string | null;
  reset(): void;
}

export function useCreateAuditDestination(scope: AuditScope): CreateAuditDestinationState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (request: CreateDestinationRequest) => auditApi.createDestination(scope, request),
    onSuccess: (destination) => {
      queryClient.setQueryData<DestinationsResponse>(
        auditQueryKeys.destinations(scope),
        (response) =>
          response
            ? { ...response, destinations: [...response.destinations, destination] }
            : response
      );
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: auditQueryKeys.scope(scope) }),
  });

  return {
    create: (request, onCreated) =>
      mutate(request, { onSuccess: (destination) => onCreated?.(destination) }),
    pending: isPending,
    refusal: error instanceof AuditRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
