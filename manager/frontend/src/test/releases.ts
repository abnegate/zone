import type {
  Lookup,
  OptionsResponse,
  PipelineResponse,
  PipelineRun,
  Preview,
  Release,
  ReleasesResponse,
  SourceOptions,
} from '../features/releases/types';

export const SOURCE_ID = '3a9e1d27-5b4c-4f8e-9a1b-2c3d4e5f6a7b';
export const REPOSITORY = 'acme/zone';
export const REPOSITORY_URL = 'https://github.com/acme/zone';

export function pipelineRun(overrides: Partial<PipelineRun> = {}): PipelineRun {
  return {
    id: 7,
    workflow_id: 3,
    attempt: 1,
    name: 'Release',
    path: '.github/workflows/release.yml',
    state: 'succeeded',
    head_sha: 'a'.repeat(40),
    created_at: '2026-11-13T09:00:05Z',
    started_at: '2026-11-13T09:00:10Z',
    updated_at: '2026-11-13T09:05:00Z',
    url: `${REPOSITORY_URL}/actions/runs/7`,
    ...overrides,
  };
}

export interface ReleaseShape extends Partial<Omit<Release, 'pipeline'>> {
  lookup?: Lookup;
  runs?: PipelineRun[];
  checked_at?: string;
}

export function release(shape: ReleaseShape = {}): Release {
  const {
    lookup = 'complete',
    runs = [pipelineRun()],
    checked_at = '2026-11-13T09:06:00Z',
    ...overrides
  } = shape;
  const id = overrides.id ?? 42;
  const tag = overrides.tag ?? 'v1.2.3';
  const published = overrides.published_at ?? '2026-11-13T09:00:00Z';
  const url = `${REPOSITORY_URL}/releases/tag/${tag}`;
  return {
    source_id: SOURCE_ID,
    repository: REPOSITORY,
    repository_url: REPOSITORY_URL,
    name: tag,
    url,
    prerelease: false,
    pulls: [],
    state: 'succeeded',
    succeeded: true,
    complete: true,
    blockers: [],
    ...overrides,
    id,
    tag,
    published_at: published,
    pipeline: {
      release: { repository: REPOSITORY, id, tag, published_at: published, url },
      lookup,
      runs,
      checked_at,
    },
  };
}

export function listing(releases: Release[]): ReleasesResponse {
  return {
    generated_at: '2026-11-13T09:06:00Z',
    stale: false,
    partial: false,
    window_days: 7,
    ttl_secs: 120,
    sources: [],
    releases,
    warnings: [],
  };
}

export function verdict(
  subject: Release,
  overrides: Partial<PipelineResponse> = {}
): PipelineResponse {
  const { pipeline, state, succeeded, complete, blockers } = subject;
  return { pipeline, state, succeeded, complete, blockers, ...overrides };
}

export const options: OptionsResponse = {
  generated_at: '2026-11-13T09:06:00Z',
  sources: [
    {
      source_id: SOURCE_ID,
      repository: REPOSITORY,
      repository_url: REPOSITORY_URL,
      default_branch: 'main',
      latest_tag: 'v1.2.3',
      next_tag: 'v1.2.4',
      previous_tags: ['v1.2.3', 'v1.2.2'],
      lines: [
        {
          prefix: null,
          latest_tag: 'v1.2.3',
          next_tag: 'v1.2.4',
          previous_tags: ['v1.2.3', 'v1.2.2'],
        },
      ],
    },
  ],
  warnings: [],
};

export const PREFIXED_SOURCE_ID = '5c2f8a91-7d3e-4b6a-9c1d-2e3f4a5b6c7d';

export const prefixedSource: SourceOptions = {
  source_id: PREFIXED_SOURCE_ID,
  repository: 'acme/crates',
  repository_url: 'https://github.com/acme/crates',
  default_branch: 'main',
  latest_tag: 'abnegate-vcs/v0.2.1',
  next_tag: 'abnegate-vcs/v0.2.2',
  previous_tags: ['abnegate-vcs/v0.3.0-rc.1', 'abnegate-vcs/v0.2.1', 'abnegate-vcs/v0.2.0'],
  lines: [
    {
      prefix: 'abnegate-vcs',
      latest_tag: 'abnegate-vcs/v0.2.1',
      next_tag: 'abnegate-vcs/v0.2.2',
      previous_tags: ['abnegate-vcs/v0.3.0-rc.1', 'abnegate-vcs/v0.2.1', 'abnegate-vcs/v0.2.0'],
    },
    {
      prefix: 'abnegate-llm',
      latest_tag: 'abnegate-llm/v1.0.0',
      next_tag: 'abnegate-llm/v1.0.1',
      previous_tags: ['abnegate-llm/v1.0.0'],
    },
  ],
};

export const preview: Preview = {
  source_id: SOURCE_ID,
  repository: REPOSITORY,
  tag: 'v1.2.4',
  name: 'v1.2.4',
  body: '## What changed',
  base_tag: 'v1.2.3',
  target: 'b'.repeat(40),
  prerelease: false,
  pulls: [],
  digest: 'c'.repeat(64),
};
