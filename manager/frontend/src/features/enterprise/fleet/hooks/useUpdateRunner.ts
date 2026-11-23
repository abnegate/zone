import { useMutation, useQueryClient } from '@tanstack/react-query';
import { fleetApi } from '../api';
import { FleetRefusal } from '../FleetRefusal';
import { fleetQueryKeys } from '../queryKeys';
import type { RunnerResponse, RunnersResponse, UpdateRunnerRequest } from '../types';

interface RunnerChange {
  runnerId: string;
  request: UpdateRunnerRequest;
}

export interface UpdateRunnerState {
  update(
    runnerId: string,
    request: UpdateRunnerRequest,
    onUpdated?: (runner: RunnerResponse) => void,
    onRefused?: (refusal: FleetRefusal) => void
  ): void;
  pending: boolean;
  refusal: FleetRefusal | null;
  error: string | null;
  reset(): void;
}

export function useUpdateRunner(): UpdateRunnerState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: ({ runnerId, request }: RunnerChange) => fleetApi.updateRunner(runnerId, request),
    onSuccess: (runner) => {
      queryClient.setQueryData<RunnersResponse>(fleetQueryKeys.runners, (listed) =>
        listed
          ? {
              ...listed,
              runners: listed.runners.map((row) => (row.id === runner.id ? runner : row)),
            }
          : listed
      );
      void queryClient.invalidateQueries({ queryKey: fleetQueryKeys.runners });
      void queryClient.invalidateQueries({ queryKey: fleetQueryKeys.labels });
    },
  });

  return {
    update: (runnerId, request, onUpdated, onRefused) =>
      mutate(
        { runnerId, request },
        {
          onSuccess: (runner) => onUpdated?.(runner),
          onError: (failure) => {
            if (failure instanceof FleetRefusal) onRefused?.(failure);
          },
        }
      ),
    pending: isPending,
    refusal: error instanceof FleetRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
