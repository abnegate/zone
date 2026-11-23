import { useMutation, useQueryClient } from '@tanstack/react-query';
import { fleetApi } from '../api';
import { FleetRefusal } from '../FleetRefusal';
import { fleetQueryKeys } from '../queryKeys';
import type { FleetPolicyResponse, UpdateFleetPolicyRequest } from '../types';

export interface UpdateFleetPolicyState {
  update(
    request: UpdateFleetPolicyRequest,
    onUpdated?: (policy: FleetPolicyResponse) => void,
    onRefused?: (refusal: FleetRefusal) => void
  ): void;
  pending: boolean;
  refusal: FleetRefusal | null;
  error: string | null;
  reset(): void;
}

export function useUpdateFleetPolicy(): UpdateFleetPolicyState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (request: UpdateFleetPolicyRequest) => fleetApi.updatePolicy(request),
    onSuccess: (policy) => {
      queryClient.setQueryData(fleetQueryKeys.policy, policy);
      void queryClient.invalidateQueries({ queryKey: fleetQueryKeys.runners });
      void queryClient.invalidateQueries({ queryKey: fleetQueryKeys.labels });
    },
  });

  return {
    update: (request, onUpdated, onRefused) =>
      mutate(request, {
        onSuccess: (policy) => onUpdated?.(policy),
        onError: (failure) => {
          if (failure instanceof FleetRefusal) onRefused?.(failure);
        },
      }),
    pending: isPending,
    refusal: error instanceof FleetRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
