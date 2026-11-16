import { type QueryClient, queryOptions } from '@tanstack/react-query';
import { releasesApi } from '../../../api/releases';
import { NO_WORKSPACE } from '../../github/errors';
import type { ReleasesResponse } from '../types';
import { newerPipelines } from '../utils/pipeline';
import { releaseWindowKey } from './keys';

export async function readReleases(
  queryClient: QueryClient,
  workspaceId: string,
  days: number | undefined,
  refresh: boolean
): Promise<ReleasesResponse> {
  const listing = await releasesApi.list(workspaceId, refresh ? { days, refresh } : { days });
  const cached = queryClient.getQueryData<ReleasesResponse>(releaseWindowKey(workspaceId, days));
  return newerPipelines(listing, cached);
}

export function releaseListQuery(
  queryClient: QueryClient,
  workspaceId: string | undefined,
  days?: number
) {
  return queryOptions({
    queryKey: releaseWindowKey(workspaceId, days),
    queryFn: () => {
      if (!workspaceId) throw new Error(NO_WORKSPACE);
      return readReleases(queryClient, workspaceId, days, false);
    },
  });
}
