import { skipToken, useQuery } from '@tanstack/react-query';
import { pullRequestsApi } from '../../../api/pullRequests';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { retryRead } from '../../github';
import { pullRequestKeys } from '../queryKeys';
import type { Diff, PullRequestRevision } from '../types';
import { type PullRequestArtifact, useHeadArtifact } from './artifact';

export const DIFF_FAILED = 'Failed to load pull request diff';

export function usePullRequestDiff(
  pullRequest: PullRequestRevision | null,
  enabled: boolean
): PullRequestArtifact<Diff> {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const head = pullRequest?.head;

  const result = useQuery({
    queryKey: pullRequestKeys.diff(workspaceId, pullRequest),
    queryFn:
      enabled && workspaceId && pullRequest && head
        ? ({ signal }) => pullRequestsApi.diff(workspaceId, pullRequest, head, signal)
        : skipToken,
    staleTime: Number.POSITIVE_INFINITY,
    retry: retryRead,
  });

  return useHeadArtifact(result, DIFF_FAILED, workspaceId);
}
