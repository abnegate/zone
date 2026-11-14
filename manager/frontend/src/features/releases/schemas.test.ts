import { describe, expect, it } from 'bun:test';
import {
  CreatedResponseSchema,
  CreatedSchema,
  CreateReleaseRequestSchema,
  IncludedPullSchema,
  OptionsResponseSchema,
  PIPELINE_BLOCKER_CODES,
  PIPELINE_STATES,
  PipelineBlockerSchema,
  PipelineResponseSchema,
  PipelineRunSchema,
  PipelineStateSchema,
  PreviewRequestSchema,
  PreviewResponseSchema,
  RefusalSchema,
  ReleaseIdentitySchema,
  ReleaseSchema,
  ReleasesResponseSchema,
} from './schemas';

const identity = {
  repository: 'acme/zone',
  id: 42,
  tag: 'v1.2.3',
  published_at: '2026-11-12T09:00:00Z',
  url: 'https://github.com/acme/zone/releases/tag/v1.2.3',
};

const run = {
  id: 7,
  workflow_id: 3,
  attempt: 1,
  name: 'Release',
  path: '.github/workflows/release.yml',
  state: 'running',
  head_sha: 'a'.repeat(40),
  created_at: '2026-11-12T09:00:05Z',
  started_at: '2026-11-12T09:00:10Z',
  updated_at: '2026-11-12T09:01:00Z',
  url: 'https://github.com/acme/zone/actions/runs/7',
};

const pipeline = {
  release: identity,
  lookup: 'complete',
  runs: [run],
  checked_at: '2026-11-12T09:01:05Z',
};

const release = {
  source_id: 'source-1',
  repository: 'acme/zone',
  repository_url: 'https://github.com/acme/zone',
  id: 42,
  tag: 'v1.2.3',
  name: 'v1.2.3',
  url: identity.url,
  published_at: identity.published_at,
  prerelease: false,
  pulls: [{ number: 12, url: 'https://github.com/acme/zone/pull/12', title: 'Fix it' }],
  pipeline,
  state: 'running',
  succeeded: false,
  complete: true,
  blockers: [{ code: 'runs_active', detail: '1 of 1 release workflow runs are still running' }],
};

const source = {
  source_id: 'source-1',
  name: 'Zone',
  repository: 'acme/zone',
  repository_url: 'https://github.com/acme/zone',
  freshness: 'refreshed',
  generated_at: '2026-11-12T09:01:05Z',
  warning: null,
};

const listing = {
  generated_at: '2026-11-12T09:01:05Z',
  stale: false,
  partial: false,
  window_days: 7,
  ttl_secs: 120,
  sources: [source],
  releases: [release],
  warnings: [],
};

describe('releases listing', () => {
  it('accepts the listing the server sends', () => {
    const parsed = ReleasesResponseSchema.parse(listing);
    expect(parsed.releases[0].pipeline.runs[0].state).toBe('running');
    expect(parsed.releases[0].pulls[0].number).toBe(12);
    expect(parsed.sources[0].freshness).toBe('refreshed');
  });

  it('defaults absent warnings, pulls and blockers to empty lists', () => {
    const { warnings: _warnings, ...bare } = listing;
    const { pulls: _pulls, blockers: _blockers, ...minimal } = release;
    const parsed = ReleasesResponseSchema.parse({ ...bare, releases: [minimal] });
    expect(parsed.warnings).toEqual([]);
    expect(parsed.releases[0].pulls).toEqual([]);
    expect(parsed.releases[0].blockers).toEqual([]);
  });

  it('accepts a run the server sent without a start time', () => {
    const { started_at: _started, ...queued } = run;
    const parsed = ReleaseSchema.parse({
      ...release,
      pipeline: { ...pipeline, runs: [{ ...queued, state: 'queued' }] },
    });
    expect(parsed.pipeline.runs[0].started_at).toBeUndefined();
  });

  it('accepts a release without a name', () => {
    expect(ReleaseSchema.parse({ ...release, name: null }).name).toBeNull();
  });

  it('rejects a release without its pipeline', () => {
    const { pipeline: _pipeline, ...bare } = release;
    expect(ReleaseSchema.safeParse(bare).success).toBe(false);
  });
});

describe('pipeline state', () => {
  it.each([...PIPELINE_STATES])('accepts the kebab-case state %s', (state) => {
    expect(PipelineStateSchema.parse(state)).toBe(state);
  });

  it.each(['timed_out', 'action_required', 'paused', '', null, 7])(
    'reads a run in state %p as unknown',
    (state) => {
      expect(PipelineStateSchema.safeParse(state).success).toBe(false);
      expect(PipelineRunSchema.parse({ ...run, state }).state).toBe('unknown');
    }
  );

  it('keeps a release whose state, lookup or blocker is newer than this client', () => {
    const parsed = ReleaseSchema.parse({
      ...release,
      state: 'deferred',
      pipeline: { ...pipeline, lookup: 'partial', runs: [{ ...run, state: 'deferred' }] },
      blockers: [{ code: 'runs_deferred', detail: 'Deferred' }],
    });
    expect(parsed.state).toBe('unknown');
    expect(parsed.pipeline.lookup).toBe('unavailable');
    expect(parsed.pipeline.runs[0].state).toBe('unknown');
    expect(parsed.blockers[0].code).toBe('runs_deferred');
  });

  it.each([...PIPELINE_BLOCKER_CODES])('keeps the known blocker code %s', (code) => {
    expect(PipelineBlockerSchema.parse({ code, detail: 'Blocked' }).code).toBe(code);
  });
});

describe('release links', () => {
  const unsafe = ['javascript:alert(1)', 'data:text/html,<script>alert(1)</script>', '/relative'];

  it.each(unsafe)('refuses a release url of %s', (url) => {
    expect(ReleaseSchema.safeParse({ ...release, url }).success).toBe(false);
    expect(ReleaseSchema.safeParse({ ...release, repository_url: url }).success).toBe(false);
  });

  it.each(unsafe)('refuses a run, identity or pull request url of %s', (url) => {
    expect(PipelineRunSchema.safeParse({ ...run, url }).success).toBe(false);
    expect(ReleaseIdentitySchema.safeParse({ ...pipeline.release, url }).success).toBe(false);
    expect(IncludedPullSchema.safeParse({ number: 1, url, title: 'Change' }).success).toBe(false);
  });

  it.each(unsafe)('refuses an options, created or source url of %s', (url) => {
    expect(
      OptionsResponseSchema.safeParse({
        generated_at: '2026-11-12T09:00:00Z',
        sources: [
          {
            source_id: 'source-1',
            repository: 'acme/zone',
            repository_url: url,
            default_branch: 'main',
            latest_tag: null,
            next_tag: 'v0.1.0',
          },
        ],
      }).success
    ).toBe(false);
    expect(
      CreatedSchema.safeParse({
        source_id: 'source-1',
        id: 1,
        tag: 'v1.0.0',
        name: null,
        url,
        published_at: '2026-11-13T10:00:00Z',
      }).success
    ).toBe(false);
    expect(
      ReleasesResponseSchema.safeParse({
        ...listing,
        sources: [{ ...source, repository_url: url }],
      }).success
    ).toBe(false);
  });
});

describe('pipeline response', () => {
  it('accepts a pipeline with its verdict', () => {
    const parsed = PipelineResponseSchema.parse({
      pipeline: { ...pipeline, lookup: 'pending', runs: [] },
      state: 'unknown',
      succeeded: false,
      complete: false,
      blockers: [{ code: 'lookup_pending', detail: 'No run yet' }],
    });
    expect(parsed.pipeline.lookup).toBe('pending');
    expect(parsed.blockers[0].code).toBe('lookup_pending');
  });
});

describe('release options', () => {
  const options = {
    source_id: 'source-1',
    repository: 'acme/zone',
    repository_url: 'https://github.com/acme/zone',
    default_branch: 'main',
    latest_tag: 'v1.2.2',
    next_tag: 'v1.2.3',
    previous_tags: ['v1.2.2', 'v1.2.1'],
  };

  it('accepts the options the server sends', () => {
    const parsed = OptionsResponseSchema.parse({
      generated_at: '2026-11-12T09:00:00Z',
      sources: [options],
      warnings: ['One source could not be read'],
    });
    expect(parsed.sources[0].next_tag).toBe('v1.2.3');
    expect(parsed.warnings).toEqual(['One source could not be read']);
  });

  it('accepts a repository with no tags yet', () => {
    const parsed = OptionsResponseSchema.parse({
      generated_at: '2026-11-12T09:00:00Z',
      sources: [{ ...options, latest_tag: null, next_tag: 'v0.1.0', previous_tags: [] }],
    });
    expect(parsed.sources[0].latest_tag).toBeNull();
    expect(parsed.sources[0].lines).toEqual([]);
    expect(parsed.warnings).toEqual([]);
  });

  it('accepts each tag line, the unprefixed one with a null prefix', () => {
    const lines = [
      {
        prefix: 'abnegate-vcs',
        latest_tag: 'abnegate-vcs/v0.2.1',
        next_tag: 'abnegate-vcs/v0.2.2',
        previous_tags: ['abnegate-vcs/v0.2.1'],
      },
      { prefix: null, latest_tag: null, next_tag: 'v0.1.0', previous_tags: [] },
    ];
    const parsed = OptionsResponseSchema.parse({
      generated_at: '2026-11-12T09:00:00Z',
      sources: [{ ...options, lines }],
    });
    expect(parsed.sources[0].lines).toEqual(lines);
  });

  it('rejects a line without its next tag', () => {
    expect(
      OptionsResponseSchema.safeParse({
        generated_at: '2026-11-12T09:00:00Z',
        sources: [{ ...options, lines: [{ prefix: null, latest_tag: null, previous_tags: [] }] }],
      }).success
    ).toBe(false);
  });
});

describe('release preview', () => {
  const preview = {
    source_id: 'source-1',
    repository: 'acme/zone',
    tag: 'v1.2.3',
    name: 'v1.2.3',
    body: "## What's Changed\n* Fix it by @someone in https://github.com/acme/zone/pull/12",
    base_tag: 'v1.2.2',
    target: 'b'.repeat(40),
    prerelease: false,
    pulls: [{ number: 12, url: 'https://github.com/acme/zone/pull/12', title: 'Fix it' }],
    digest: 'c'.repeat(64),
  };

  it('accepts the preview the server sends', () => {
    expect(PreviewResponseSchema.parse({ preview }).preview).toEqual(preview);
  });

  it('carries whether the tag makes a pre-release', () => {
    const candidate = { ...preview, tag: 'v1.2.3-rc.1', prerelease: true };
    expect(PreviewResponseSchema.parse({ preview: candidate }).preview.prerelease).toBe(true);
    const { prerelease: _omitted, ...unsaid } = preview;
    expect(PreviewResponseSchema.safeParse({ preview: unsaid }).success).toBe(false);
  });

  it('accepts a first release with no base tag', () => {
    expect(
      PreviewResponseSchema.parse({ preview: { ...preview, base_tag: null } }).preview.base_tag
    ).toBeNull();
  });

  it('needs a tag to preview', () => {
    const request = { source_id: 'source-1', tag: '  ', expected_latest_tag: null };
    expect(PreviewRequestSchema.safeParse(request).success).toBe(false);
  });

  it('needs the digest of the reviewed preview to create', () => {
    const request = {
      source_id: 'source-1',
      tag: 'v1.2.3',
      expected_latest_tag: 'v1.2.2',
      prerelease: false,
    };
    expect(CreateReleaseRequestSchema.safeParse(request).success).toBe(false);
    expect(CreateReleaseRequestSchema.safeParse({ ...request, digest: 'd' }).success).toBe(true);
  });
});

describe('created release', () => {
  it('accepts the release the server published', () => {
    const created = {
      source_id: 'source-1',
      id: 43,
      tag: 'v1.2.3',
      name: 'v1.2.3',
      url: 'https://github.com/acme/zone/releases/tag/v1.2.3',
      published_at: '2026-11-13T10:00:00Z',
    };
    expect(CreatedResponseSchema.parse({ release: created }).release).toEqual(created);
  });
});

describe('release refusals', () => {
  it.each(['latest_tag_moved', 'preview_stale', 'release_in_progress'])(
    'accepts the %s refusal',
    (code) => {
      const parsed = RefusalSchema.parse({ success: false, error: 'Refused', code });
      expect(parsed.code).toBe(code as typeof parsed.code);
    }
  );

  it('accepts a taken tag with or without its tag object', () => {
    const taken = { success: false, error: 'Taken', code: 'tag_exists', tag: 'v1.2.3' };
    expect(RefusalSchema.parse(taken)).toMatchObject({ code: 'tag_exists', tag: 'v1.2.3' });
    expect(RefusalSchema.parse({ ...taken, tag_object: 'f'.repeat(40) })).toMatchObject({
      tag_object: 'f'.repeat(40),
    });
  });

  it('accepts a tag that is not newer than its line', () => {
    const parsed = RefusalSchema.parse({
      success: false,
      error: 'pkg/v1.2.3 is not newer than the latest tag, pkg/v1.3.0. Choose a newer tag.',
      code: 'tag_not_newer',
      tag: 'pkg/v1.2.3',
      latest_tag: 'pkg/v1.3.0',
    });
    expect(parsed).toMatchObject({ tag: 'pkg/v1.2.3', latest_tag: 'pkg/v1.3.0' });
  });

  it('accepts a tag that already has a release', () => {
    const parsed = RefusalSchema.parse({
      success: false,
      error: 'Release 91 already uses the tag v1.2.3. Choose another tag.',
      code: 'release_exists',
      tag: 'v1.2.3',
      release_id: 91,
    });
    expect(parsed).toMatchObject({ tag: 'v1.2.3', release_id: 91 });
  });

  it.each([
    { code: 'tag_exists' },
    { code: 'tag_not_newer', tag: 'v1.2.3' },
    { code: 'release_exists', tag: 'v1.2.3' },
  ])('rejects a tag refusal that does not name what it refused: %j', (refusal) => {
    expect(RefusalSchema.safeParse({ error: 'Refused', ...refusal }).success).toBe(false);
  });

  const reconciliation = {
    success: false,
    error: 'The release was only partly created and needs reconciling on GitHub',
    code: 'release_manual_reconciliation_required',
    tag: 'v1.2.3',
    tag_object: 'e'.repeat(40),
    reference_created: true,
    release_id: null,
    release_exists: false,
    marker: '0b9f1c52-6d4e-4a8b-9f2e-1c3d5e7f9a0b',
  };

  it('carries what is left to reconcile by hand', () => {
    expect(RefusalSchema.parse(reconciliation)).toMatchObject({
      tag_object: 'e'.repeat(40),
      reference_created: true,
      release_exists: false,
      marker: reconciliation.marker,
    });
  });

  it('accepts a reference that could not be read back and a draft release', () => {
    expect(
      RefusalSchema.parse({ ...reconciliation, reference_created: null, release_id: 91 })
    ).toMatchObject({ reference_created: null, release_id: 91 });
  });

  it.each(['release_exists', 'marker'])('rejects a reconciliation without %s', (field) => {
    const { [field as keyof typeof reconciliation]: _omitted, ...partial } = reconciliation;
    expect(RefusalSchema.safeParse(partial).success).toBe(false);
  });

  it('rejects a reconciliation refusal that does not say what was written', () => {
    expect(
      RefusalSchema.safeParse({
        error: 'The release needs manual reconciliation',
        code: 'release_manual_reconciliation_required',
      }).success
    ).toBe(false);
  });

  it('rejects a code it does not know', () => {
    expect(RefusalSchema.safeParse({ error: 'Refused', code: 'head_moved' }).success).toBe(false);
  });
});
