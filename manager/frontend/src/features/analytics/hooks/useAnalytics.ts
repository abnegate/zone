import { partialMatchKey, useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import { analyticsApi, LOAD_FAILED } from '../../../api/analytics';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { analyticsKeys } from '../queryKeys';
import type { Overview, TimePeriod } from '../types';

export const STALE_MS = 60_000;

export const NO_WORKSPACE = 'No workspace selected';

export interface Analytics {
  overview: Overview | undefined;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  missingWorkspace: boolean;
  retry(): void;
}

function messageOf(error: unknown): string | null {
  if (!error) return null;
  return error instanceof Error && error.message ? error.message : LOAD_FAILED;
}

export function useAnalytics(period: TimePeriod): Analytics {
  const { currentWorkspace, loading: workspaceLoading } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const { data, isPending, isFetching, isPlaceholderData, error, refetch } = useQuery({
    queryKey: analyticsKeys.overview(workspaceId, period),
    queryFn: ({ signal }) => {
      if (!workspaceId) throw new Error(NO_WORKSPACE);
      return analyticsApi.overview(workspaceId, period, signal);
    },
    enabled: !!workspaceId,
    staleTime: STALE_MS,
    placeholderData: (previous, previousQuery) =>
      previousQuery && partialMatchKey(previousQuery.queryKey, analyticsKeys.workspace(workspaceId))
        ? previous
        : undefined,
  });

  const retry = useCallback(() => {
    void refetch();
  }, [refetch]);

  return {
    overview: data,
    loading: workspaceId ? isPending : workspaceLoading,
    refreshing: isFetching && isPlaceholderData,
    error: messageOf(error),
    missingWorkspace: !workspaceLoading && !workspaceId,
    retry,
  };
}
