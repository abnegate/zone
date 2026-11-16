import type { QueryClient } from '@tanstack/react-query';
import { PRELOADED_GC_TIME } from '../../github/constants';
import { releaseListQuery } from './listQuery';

export function preloadReleases(queryClient: QueryClient, workspaceId: string): Promise<void> {
  return queryClient.prefetchQuery({
    ...releaseListQuery(queryClient, workspaceId),
    gcTime: PRELOADED_GC_TIME,
  });
}
