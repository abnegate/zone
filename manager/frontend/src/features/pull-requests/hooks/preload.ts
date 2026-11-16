import type { QueryClient } from '@tanstack/react-query';
import { PRELOADED_GC_TIME } from '../../github/constants';
import { pullRequestListQuery } from './listQuery';

export function preloadPullRequests(queryClient: QueryClient, workspaceId: string): Promise<void> {
  return queryClient.prefetchQuery({
    ...pullRequestListQuery(workspaceId),
    gcTime: PRELOADED_GC_TIME,
  });
}
