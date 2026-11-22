import { useMutation, useQueryClient } from '@tanstack/react-query';
import { fleetApi } from '../api';
import { FleetRefusal } from '../FleetRefusal';
import { type Placement, PlacementKind, type PlacementScope } from '../placement';
import { fleetQueryKeys } from '../queryKeys';

export interface UpdateRunnerLabelsState {
  update(labels: string[], onUpdated?: (placement: Placement) => void): void;
  pending: boolean;
  refusal: FleetRefusal | null;
  error: string | null;
  reset(): void;
}

export function useUpdateRunnerLabels(scope: PlacementScope): UpdateRunnerLabelsState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (labels: string[]) => fleetApi.updateLabels(scope, { labels }),
    onSuccess: (placement) => {
      queryClient.setQueryData(fleetQueryKeys.placement(scope), placement);
      if (scope.kind === PlacementKind.Organization) {
        void queryClient.invalidateQueries({
          queryKey: fleetQueryKeys.workspaces(scope.organizationId),
        });
      }
    },
  });

  return {
    update: (labels, onUpdated) =>
      mutate(labels, { onSuccess: (placement) => onUpdated?.(placement) }),
    pending: isPending,
    refusal: error instanceof FleetRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
