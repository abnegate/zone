import { useMutation, useQueryClient } from '@tanstack/react-query';
import { fleetApi } from '../api';
import { FleetRefusal } from '../FleetRefusal';
import { fleetQueryKeys } from '../queryKeys';
import type { RunnersResponse } from '../types';

export interface RemoveRunnerState {
  remove(runnerId: string, onRemoved?: () => void): void;
  pending: boolean;
  refusal: FleetRefusal | null;
  error: string | null;
  reset(): void;
}

export function useRemoveRunner(): RemoveRunnerState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (runnerId: string) => fleetApi.removeRunner(runnerId),
    onSuccess: (_, runnerId) => {
      queryClient.setQueryData<RunnersResponse>(fleetQueryKeys.runners, (listed) =>
        listed
          ? { ...listed, runners: listed.runners.filter((runner) => runner.id !== runnerId) }
          : listed
      );
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: fleetQueryKeys.runners }),
  });

  return {
    remove: (runnerId, onRemoved) => mutate(runnerId, { onSuccess: () => onRemoved?.() }),
    pending: isPending,
    refusal: error instanceof FleetRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
