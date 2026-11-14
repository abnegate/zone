const RELEASES = 'releases';
const LIST = 'list';
const OPTIONS = 'options';
const PIPELINE = 'pipeline';

export function releasesKey(workspaceId?: string) {
  return [RELEASES, workspaceId] as const;
}

export function releaseListingsKey(workspaceId?: string) {
  return [...releasesKey(workspaceId), LIST] as const;
}

export function releaseWindowKey(workspaceId?: string, days?: number) {
  return [...releaseListingsKey(workspaceId), days ?? null] as const;
}

export function releaseOptionsKey(workspaceId?: string) {
  return [...releasesKey(workspaceId), OPTIONS] as const;
}

export function releasePipelineKey(workspaceId: string | undefined, sourceId: string, id: number) {
  return [...releasesKey(workspaceId), PIPELINE, sourceId, id] as const;
}
