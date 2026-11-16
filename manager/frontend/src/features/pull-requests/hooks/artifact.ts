import { type UseQueryResult, useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';
import { errorMessage } from '../../github';
import { PullRequestRefusal } from '../PullRequestRefusal';
import { pullRequestKeys } from '../queryKeys';
import { RefusalCodeSchema } from '../schemas';

export interface PullRequestArtifact<T> {
  data: T | undefined;
  loading: boolean;
  error: string | null;
  refusal: PullRequestRefusal | null;
}

export function artifactOf<T>(result: UseQueryResult<T>, fallback: string): PullRequestArtifact<T> {
  return {
    data: result.data,
    loading: result.isLoading,
    error: errorMessage(result.error, fallback),
    refusal: result.error instanceof PullRequestRefusal ? result.error : null,
  };
}

export function useHeadArtifact<T>(
  result: UseQueryResult<T>,
  fallback: string,
  workspaceId: string | undefined
): PullRequestArtifact<T> {
  const queryClient = useQueryClient();
  const artifact = artifactOf(result, fallback);
  const moved = artifact.refusal?.code === RefusalCodeSchema.enum.head_moved;

  useEffect(() => {
    if (!moved || !workspaceId) return;
    void queryClient.invalidateQueries({ queryKey: pullRequestKeys.list(workspaceId) });
  }, [moved, workspaceId, queryClient]);

  return artifact;
}
