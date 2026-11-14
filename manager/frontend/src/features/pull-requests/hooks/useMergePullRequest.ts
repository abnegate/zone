import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { pullRequestsApi } from '../../../api/pullRequests';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { errorMessage, NO_WORKSPACE, outcomeUnknown } from '../../github';
import { releasesKey } from '../../releases';
import { PullRequestRefusal } from '../PullRequestRefusal';
import { pullRequestListings } from '../queryKeys';
import { RefusalCodeSchema } from '../schemas';
import type { Merged, MergeMethod, PullRequestRevision, RefusalCode } from '../types';

export const MERGE_FAILED = 'Failed to merge pull request';

export const MERGE_OUTCOME_UNKNOWN =
  'The merge stopped before it reported back, so it may have happened. Check the pull request on GitHub before trying again.';

export const NO_HEAD = 'The pull request has no head commit to merge';

const SETTLED_CODES: readonly RefusalCode[] = [
  RefusalCodeSchema.enum.merged,
  RefusalCodeSchema.enum.closed,
];

interface Merge {
  workspaceId: string;
  pullRequest: PullRequestRevision;
  method?: MergeMethod;
}

export interface MergePullRequest {
  merge(pullRequest: PullRequestRevision, method?: MergeMethod): Promise<Merged>;
  merging: boolean;
  error: string | null;
  refusal: PullRequestRefusal | null;
  settled: boolean;
  unknown: boolean;
  reset(): void;
}

export function useMergePullRequest(): MergePullRequest {
  const queryClient = useQueryClient();
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const { mutateAsync, isPending, error, reset } = useMutation({
    mutationFn: ({ workspaceId: target, pullRequest, method }: Merge) => {
      if (!pullRequest.head) throw new Error(NO_HEAD);
      return pullRequestsApi.merge(target, pullRequest, {
        expected_head: pullRequest.head,
        method,
      });
    },
    onSettled: (_merged, failure, { workspaceId: target }) => {
      void queryClient.invalidateQueries(pullRequestListings(target));
      if (!failure || outcomeUnknown(failure)) {
        void queryClient.invalidateQueries({ queryKey: releasesKey(target) });
      }
    },
  });

  const merge = useCallback(
    (pullRequest: PullRequestRevision, method?: MergeMethod): Promise<Merged> => {
      if (!workspaceId) return Promise.reject(new Error(NO_WORKSPACE));
      return mutateAsync({ workspaceId, pullRequest, method });
    },
    [mutateAsync, workspaceId]
  );

  const refusal = error instanceof PullRequestRefusal ? error : null;
  const unknown = outcomeUnknown(error);

  return {
    merge,
    merging: isPending,
    error: unknown ? MERGE_OUTCOME_UNKNOWN : errorMessage(error, MERGE_FAILED),
    refusal,
    settled: refusal !== null && SETTLED_CODES.includes(refusal.code),
    unknown,
    reset,
  };
}
