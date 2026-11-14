import { useQuery } from '@tanstack/react-query';
import { useCallback, useEffect, useState } from 'react';
import { pullRequestsApi } from '../../../api/pullRequests';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import {
  errorMessage,
  gatewayFailed,
  NO_WORKSPACE,
  retryDelay,
  retryPolled,
  type SourceStatus,
  useRefresh,
} from '../../github';
import { pullRequestKeys } from '../queryKeys';
import type { PullRequest, PullRequestsResponse } from '../types';

export const LIST_POLL_MS = 30_000;

export const LOAD_FAILED = 'Failed to load pull requests';

const MILLISECONDS = 1_000;

const NO_PULL_REQUESTS: readonly PullRequest[] = [];

const NO_SOURCES: readonly SourceStatus[] = [];

const NO_WARNINGS: readonly string[] = [];

export interface PullRequests {
  pullRequests: readonly PullRequest[];
  sources: readonly SourceStatus[];
  warnings: readonly string[];
  generatedAt: string | null;
  stale: boolean;
  partial: boolean;
  loading: boolean;
  error: string | null;
  unavailable: boolean;
  refreshing: boolean;
  floored: boolean;
  refreshable: boolean;
  refresh(): void;
}

export function floorEnd(listing: PullRequestsResponse, receivedAt: number): number {
  const generatedAt = Date.parse(listing.generated_at);
  const start = Number.isNaN(generatedAt) ? receivedAt : Math.min(generatedAt, receivedAt);
  return start + listing.refresh_floor_secs * MILLISECONDS;
}

function useBefore(deadline: number | null): boolean {
  const [now, setNow] = useState(Date.now);

  useEffect(() => {
    if (deadline === null) return;
    const remaining = deadline - Date.now();
    if (remaining <= 0) {
      setNow(Date.now());
      return;
    }
    const timer = setTimeout(() => setNow(Date.now()), remaining);
    return () => clearTimeout(timer);
  }, [deadline]);

  return deadline !== null && deadline > now;
}

export function usePullRequests(): PullRequests {
  const { currentWorkspace, loading: workspaceLoading } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const { data, dataUpdatedAt, isLoading, error, failureReason } = useQuery({
    queryKey: pullRequestKeys.list(workspaceId),
    queryFn: ({ signal }) => {
      if (!workspaceId) throw new Error(NO_WORKSPACE);
      return pullRequestsApi.list(workspaceId, false, signal);
    },
    enabled: !!workspaceId,
    refetchInterval: LIST_POLL_MS,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    retry: retryPolled,
    retryDelay,
  });

  const { refreshing, failure: refreshFailure, start } = useRefresh(dataUpdatedAt);

  const floored = useBefore(data ? floorEnd(data, dataUpdatedAt) : null);
  const refreshable = !!workspaceId && !!data && !floored && !refreshing;

  const refresh = useCallback(() => {
    if (workspaceId && refreshable) {
      start(pullRequestKeys.list(workspaceId), () => pullRequestsApi.list(workspaceId, true));
    }
  }, [start, refreshable, workspaceId]);

  const failure = error ?? refreshFailure ?? failureReason;

  return {
    pullRequests: data?.pull_requests ?? NO_PULL_REQUESTS,
    sources: data?.sources ?? NO_SOURCES,
    warnings: data?.warnings ?? NO_WARNINGS,
    generatedAt: data?.generated_at ?? null,
    stale: data?.stale ?? false,
    partial: data?.partial ?? false,
    loading: workspaceId ? isLoading : workspaceLoading,
    error: errorMessage(error ?? refreshFailure, LOAD_FAILED),
    unavailable: gatewayFailed(failure),
    refreshing,
    floored,
    refreshable,
    refresh,
  };
}
