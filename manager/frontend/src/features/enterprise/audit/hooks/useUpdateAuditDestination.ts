import { useMutation, useQueryClient } from '@tanstack/react-query';
import { AuditRefusal } from '../AuditRefusal';
import { auditApi } from '../api';
import { auditQueryKeys } from '../queryKeys';
import type {
  AuditScope,
  DestinationResponse,
  DestinationsResponse,
  UpdateDestinationRequest,
} from '../types';

interface DestinationUpdate {
  destinationId: string;
  request: UpdateDestinationRequest;
}

export interface UpdateAuditDestinationState {
  update(
    destinationId: string,
    request: UpdateDestinationRequest,
    onUpdated?: (destination: DestinationResponse) => void
  ): void;
  pending: boolean;
  refusal: AuditRefusal | null;
  error: string | null;
  reset(): void;
}

export function useUpdateAuditDestination(scope: AuditScope): UpdateAuditDestinationState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: ({ destinationId, request }: DestinationUpdate) =>
      auditApi.updateDestination(scope, destinationId, request),
    onSuccess: (destination) => {
      queryClient.setQueryData<DestinationsResponse>(
        auditQueryKeys.destinations(scope),
        (response) =>
          response
            ? {
                ...response,
                destinations: response.destinations.map((current) =>
                  current.id === destination.id ? destination : current
                ),
              }
            : response
      );
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: auditQueryKeys.scope(scope) }),
  });

  return {
    update: (destinationId, request, onUpdated) =>
      mutate({ destinationId, request }, { onSuccess: (destination) => onUpdated?.(destination) }),
    pending: isPending,
    refusal: error instanceof AuditRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
