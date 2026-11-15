import type { Tint } from '../../github';
import type {
  Lookup,
  PipelineResponse,
  PipelineState,
  Release,
  ReleasePipeline,
  ReleasesResponse,
} from '../types';

export const PIPELINE_WATCH_MS = 30 * 60 * 1000;

const PENDING: Lookup = 'pending';

const ACTIVE_STATES: ReadonlySet<PipelineState> = new Set<PipelineState>(['queued', 'running']);

export const PIPELINE_TINTS: Readonly<Record<PipelineState, Tint>> = {
  'action-required': 'warning',
  cancelled: 'neutral',
  failed: 'destructive',
  neutral: 'neutral',
  queued: 'info',
  running: 'info',
  skipped: 'neutral',
  stale: 'warning',
  succeeded: 'success',
  'timed-out': 'destructive',
  unknown: 'outline',
};

export const PIPELINE_LABELS: Readonly<Record<PipelineState, string>> = {
  'action-required': 'Action required',
  cancelled: 'Cancelled',
  failed: 'Failed',
  neutral: 'Neutral',
  queued: 'Queued',
  running: 'Running',
  skipped: 'Skipped',
  stale: 'Stale',
  succeeded: 'Succeeded',
  'timed-out': 'Timed out',
  unknown: 'Unknown',
};

export function pipelineActive(pipeline: ReleasePipeline): boolean {
  return pipeline.lookup === PENDING || pipeline.runs.some((run) => ACTIVE_STATES.has(run.state));
}

export function pipelineRecent(publishedAt: string, now: number = Date.now()): boolean {
  return now - Date.parse(publishedAt) < PIPELINE_WATCH_MS;
}

export function pipelineWatched(release: Release, now: number = Date.now()): boolean {
  return pipelineRecent(release.published_at, now) && pipelineActive(release.pipeline);
}

export function verdictOf(release: Release): PipelineResponse {
  const { pipeline, state, succeeded, complete, blockers } = release;
  return { pipeline, state, succeeded, complete, blockers };
}

function checkedAt(release: Release): number {
  return Date.parse(release.pipeline.checked_at) || 0;
}

function releaseKey(release: Pick<Release, 'source_id' | 'id'>): string {
  return `${release.source_id}:${release.id}`;
}

export function newerPipelines(
  listing: ReleasesResponse,
  cached: ReleasesResponse | undefined
): ReleasesResponse {
  if (!cached) return listing;
  const known = new Map(cached.releases.map((release) => [releaseKey(release), release]));
  let kept = false;
  const releases = listing.releases.map((release) => {
    const previous = known.get(releaseKey(release));
    if (!previous || checkedAt(previous) <= checkedAt(release)) return release;
    kept = true;
    return { ...release, ...verdictOf(previous) };
  });
  return kept ? { ...listing, releases } : listing;
}

export function withPipeline(
  listing: ReleasesResponse,
  sourceId: string,
  update: PipelineResponse
): ReleasesResponse {
  const { pipeline, state, succeeded, complete, blockers } = update;
  const index = listing.releases.findIndex(
    (release) => release.source_id === sourceId && release.id === pipeline.release.id
  );
  if (index < 0) return listing;
  const releases = listing.releases.slice();
  releases[index] = { ...releases[index], pipeline, state, succeeded, complete, blockers };
  return { ...listing, releases };
}
