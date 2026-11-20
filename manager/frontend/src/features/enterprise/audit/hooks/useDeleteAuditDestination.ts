import { useMutation, useQueryClient } from '@tanstack/react-query';
import { AuditRefusal } from '../AuditRefusal';
import { auditApi } from '../api';
import { auditQueryKeys } from '../queryKeys';
import type { AuditScope, DestinationsResponse } from '../types';

export interface DeleteAuditDestinationState {
  remove(destinationId: string, onDeleted?: () => void): void;
  pending: boolean;
  refusal: AuditRefusal | null;
  error: string | null;
  reset(): void;
}

export function useDeleteAuditDestination(scope: AuditScope): DeleteAuditDestinationState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (destinationId: string) => auditApi.deleteDestination(scope, destinationId),
    onSuccess: (_, destinationId) => {
      queryClient.setQueryData<DestinationsResponse>(
        auditQueryKeys.destinations(scope),
        (response) =>
          response
            ? {
                ...response,
                destinations: response.destinations.filter(
                  (destination) => destination.id !== destinationId
                ),
              }
            : response
      );
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: auditQueryKeys.scope(scope) }),
  });

  return {
    remove: (destinationId, onDeleted) => mutate(destinationId, { onSuccess: () => onDeleted?.() }),
    pending: isPending,
    refusal: error instanceof AuditRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
