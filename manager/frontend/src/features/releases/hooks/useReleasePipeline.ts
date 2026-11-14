import { useQuery, useQueryClient } from '@tanstack/react-query';
import { releasesApi } from '../../../api/releases';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { NO_WORKSPACE, notFound, retryDelay, retryPolled } from '../../github';
import type { Release, ReleasesResponse } from '../types';
import {
  pipelineActive,
  pipelineRecent,
  pipelineWatched,
  verdictOf,
  withPipeline,
} from '../utils/pipeline';
import { releaseListingsKey, releasePipelineKey } from './keys';

export const PIPELINE_POLL_MS = 15_000;

export interface PipelineWatch {
  watching: boolean;
}

interface Answered {
  state: { error: Error | null };
}

function present(query: Answered): boolean {
  return !notFound(query.state.error);
}

export function useReleasePipeline(release: Release): PipelineWatch {
  const queryClient = useQueryClient();
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const { source_id: sourceId, id } = release;
  const watched = !!workspaceId && pipelineWatched(release);

  const { error } = useQuery({
    queryKey: releasePipelineKey(workspaceId, sourceId, id),
    queryFn: async ({ signal }) => {
      if (!workspaceId) throw new Error(NO_WORKSPACE);
      const update = await releasesApi.pipeline(workspaceId, sourceId, id, {}, signal);
      queryClient.setQueriesData<ReleasesResponse>(
        { queryKey: releaseListingsKey(workspaceId) },
        (listing) => listing && withPipeline(listing, sourceId, update)
      );
      return update;
    },
    enabled: watched,
    initialData: () => verdictOf(release),
    initialDataUpdatedAt: () => Date.parse(release.pipeline.checked_at) || 0,
    staleTime: PIPELINE_POLL_MS,
    refetchInterval: (query) => {
      if (!present(query)) return false;
      const pipeline = query.state.data?.pipeline ?? release.pipeline;
      const active = pipelineRecent(release.published_at) && pipelineActive(pipeline);
      return active ? PIPELINE_POLL_MS : false;
    },
    refetchIntervalInBackground: false,
    refetchOnMount: present,
    refetchOnWindowFocus: present,
    refetchOnReconnect: present,
    retry: retryPolled,
    retryDelay,
  });

  return { watching: watched && !notFound(error) };
}
