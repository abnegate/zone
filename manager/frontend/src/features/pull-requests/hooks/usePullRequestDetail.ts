import { skipToken, useQuery } from '@tanstack/react-query';
import { pullRequestsApi } from '../../../api/pullRequests';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { retryRead } from '../../github';
import { pullRequestKeys } from '../queryKeys';
import type { PullRequestDetail, PullRequestRevision } from '../types';
import { artifactOf, type PullRequestArtifact } from './artifact';

export const DETAIL_FAILED = 'Failed to load pull request';

export function usePullRequestDetail(
  pullRequest: PullRequestRevision | null,
  enabled = true
): PullRequestArtifact<PullRequestDetail> {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const result = useQuery({
    queryKey: pullRequestKeys.detail(workspaceId, pullRequest),
    queryFn:
      enabled && workspaceId && pullRequest
        ? ({ signal }) => pullRequestsApi.detail(workspaceId, pullRequest, signal)
        : skipToken,
    retry: retryRead,
  });

  return artifactOf(result, DETAIL_FAILED);
}
