import type { QueryClient } from '@tanstack/react-query';
import { pullRequestsApi } from '../../../api/pullRequests';
import { PRELOADED_GC_TIME } from '../../github/constants';
import { pullRequestKeys } from '../queryKeys';

export function preloadPullRequests(queryClient: QueryClient, workspaceId: string): Promise<void> {
  return queryClient.prefetchQuery({
    queryKey: pullRequestKeys.list(workspaceId),
    queryFn: ({ signal }) => pullRequestsApi.list(workspaceId, false, signal),
    gcTime: PRELOADED_GC_TIME,
  });
}
