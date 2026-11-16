import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { errorMessage, gatewayFailed, retryDelay, retryPolled, useRefresh } from '../../github';
import type { Release, ReleasesResponse } from '../types';
import { releaseWindowKey } from './keys';
import { readReleases, releaseListQuery } from './listQuery';

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

  const {
    data: listing,
    dataUpdatedAt,
    isLoading,
    error,
    failureReason,
  } = useQuery({
    ...releaseListQuery(queryClient, workspaceId, days),
    enabled: !!workspaceId,
    refetchInterval: RELEASES_POLL_MS,
    refetchIntervalInBackground: false,
    retry: retryPolled,
    retryDelay,
  });

  const { refreshing, failure: refreshFailure, start } = useRefresh(dataUpdatedAt);

  const refresh = useCallback(() => {
    if (workspaceId) {
      start(releaseWindowKey(workspaceId, days), () =>
        readReleases(queryClient, workspaceId, days, true)
      );
    }
  }, [start, queryClient, workspaceId, days]);

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
