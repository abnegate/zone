import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { releasesApi } from '../../../api/releases';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import {
  errorMessage,
  gatewayFailed,
  NO_WORKSPACE,
  retryDelay,
  retryPolled,
  useRefresh,
} from '../../github';
import type { Release, ReleasesResponse } from '../types';
import { newerPipelines } from '../utils/pipeline';
import { releaseWindowKey } from './keys';

export const RELEASES_POLL_MS = 120_000;

export const LOAD_FAILED = 'Failed to load releases';

const NO_RELEASES: readonly Release[] = [];

export interface Releases {
  listing: ReleasesResponse | undefined;
  releases: readonly Release[];
  loading: boolean;
  error: string | null;
  unavailable: boolean;
  refresh: () => void;
  refreshing: boolean;
}

export function useReleases(days?: number): Releases {
  const queryClient = useQueryClient();
  const { currentWorkspace, loading: workspaceLoading } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const read = useCallback(
    async (target: string, refresh: boolean, signal?: AbortSignal) => {
      const listing = await releasesApi.list(
        target,
        refresh ? { days, refresh } : { days },
        signal
      );
      const cached = queryClient.getQueryData<ReleasesResponse>(releaseWindowKey(target, days));
      return newerPipelines(listing, cached);
    },
    [queryClient, days]
  );

  const {
    data: listing,
    dataUpdatedAt,
    isLoading,
    error,
    failureReason,
  } = useQuery({
    queryKey: releaseWindowKey(workspaceId, days),
    queryFn: ({ signal }) => {
      if (!workspaceId) throw new Error(NO_WORKSPACE);
      return read(workspaceId, false, signal);
    },
    enabled: !!workspaceId,
    refetchInterval: RELEASES_POLL_MS,
    refetchIntervalInBackground: false,
    retry: retryPolled,
    retryDelay,
  });

  const { refreshing, failure: refreshFailure, start } = useRefresh(dataUpdatedAt);

  const refresh = useCallback(() => {
    if (workspaceId) start(releaseWindowKey(workspaceId, days), () => read(workspaceId, true));
  }, [start, read, workspaceId, days]);

  return {
    listing,
    releases: listing?.releases ?? NO_RELEASES,
    loading: workspaceId ? isLoading : workspaceLoading,
    error: errorMessage(error ?? refreshFailure, LOAD_FAILED),
    unavailable: gatewayFailed(error ?? refreshFailure ?? failureReason),
    refresh,
    refreshing,
  };
}
