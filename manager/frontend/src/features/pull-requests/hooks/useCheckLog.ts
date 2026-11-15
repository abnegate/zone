import { skipToken, useQuery } from '@tanstack/react-query';
import { pullRequestsApi } from '../../../api/pullRequests';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { retryRead } from '../../github';
import { pullRequestKeys } from '../queryKeys';
import type { CheckLog, PullRequestRevision } from '../types';
import { type PullRequestArtifact, useHeadArtifact } from './artifact';

export const LOG_FAILED = 'Failed to load check log';

export function useCheckLog(
  pullRequest: PullRequestRevision | null,
  jobId: number | null
): PullRequestArtifact<CheckLog> {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const head = pullRequest?.head;

  const result = useQuery({
    queryKey: pullRequestKeys.log(workspaceId, pullRequest, jobId),
    queryFn:
      jobId !== null && workspaceId && pullRequest && head
        ? ({ signal }) => pullRequestsApi.jobLog(workspaceId, pullRequest, jobId, head, signal)
        : skipToken,
    staleTime: Number.POSITIVE_INFINITY,
    retry: retryRead,
  });

  return useHeadArtifact(result, LOG_FAILED, workspaceId);
}
