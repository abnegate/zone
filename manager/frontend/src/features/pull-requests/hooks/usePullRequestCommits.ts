import { skipToken, useQuery } from '@tanstack/react-query';
import { pullRequestsApi } from '../../../api/pullRequests';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { retryRead } from '../../github';
import { pullRequestKeys } from '../queryKeys';
import type { CommitList, Diff, PullRequestRevision } from '../types';
import { type PullRequestArtifact, useHeadArtifact } from './artifact';

export const COMMITS_FAILED = 'Failed to load pull request commits';

export const COMMIT_DIFF_FAILED = 'Failed to load commit diff';

export function usePullRequestCommits(
  pullRequest: PullRequestRevision | null,
  enabled: boolean
): PullRequestArtifact<CommitList> {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const head = pullRequest?.head;

  const result = useQuery({
    queryKey: pullRequestKeys.commits(workspaceId, pullRequest),
    queryFn:
      enabled && workspaceId && pullRequest && head
        ? ({ signal }) => pullRequestsApi.commits(workspaceId, pullRequest, head, signal)
        : skipToken,
    staleTime: Number.POSITIVE_INFINITY,
    retry: retryRead,
  });

  return useHeadArtifact(result, COMMITS_FAILED, workspaceId);
}

export function useCommitDiff(
  pullRequest: PullRequestRevision | null,
  sha: string | null
): PullRequestArtifact<Diff> {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const head = pullRequest?.head;

  const result = useQuery({
    queryKey: pullRequestKeys.commit(workspaceId, pullRequest, sha),
    queryFn:
      sha && workspaceId && pullRequest && head
        ? ({ signal }) => pullRequestsApi.commitDiff(workspaceId, pullRequest, sha, head, signal)
        : skipToken,
    staleTime: Number.POSITIVE_INFINITY,
    retry: retryRead,
  });

  return useHeadArtifact(result, COMMIT_DIFF_FAILED, workspaceId);
}
