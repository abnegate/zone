import { type QueryKey, useMutation, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';

interface Reload {
  queryKey: QueryKey;
  load: () => Promise<unknown>;
}

export interface Refresh {
  refreshing: boolean;
  failure: Error | null;
  start(queryKey: QueryKey, load: () => Promise<unknown>): void;
}

export function useRefresh(dataUpdatedAt: number): Refresh {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, submittedAt } = useMutation({
    mutationFn: ({ load }: Reload) => load(),
    onSuccess: async (data, { queryKey }) => {
      await queryClient.cancelQueries({ queryKey });
      queryClient.setQueryData(queryKey, data);
    },
  });

  const start = useCallback(
    (queryKey: QueryKey, load: () => Promise<unknown>) => mutate({ queryKey, load }),
    [mutate]
  );

  return {
    refreshing: isPending,
    failure: error && submittedAt >= dataUpdatedAt ? error : null,
    start,
  };
}
