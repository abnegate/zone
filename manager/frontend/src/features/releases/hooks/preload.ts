import type { QueryClient } from '@tanstack/react-query';
import { releasesApi } from '../../../api/releases';
import { PRELOADED_GC_TIME } from '../../github/constants';
import { releaseWindowKey } from './keys';

export function preloadReleases(queryClient: QueryClient, workspaceId: string): Promise<void> {
  return queryClient.prefetchQuery({
    queryKey: releaseWindowKey(workspaceId),
    queryFn: ({ signal }) => releasesApi.list(workspaceId, {}, signal),
    gcTime: PRELOADED_GC_TIME,
  });
}
