import { z } from 'zod';
import { SourceStatusSchema, WebUrlSchema } from '../github';

export const PIPELINE_STATES = [
  'action-required',
  'cancelled',
  'failed',
  'neutral',
  'queued',
  'running',
  'skipped',
  'stale',
  'succeeded',
  'timed-out',
  'unknown',
] as const;

export const LOOKUPS = ['complete', 'pending', 'unavailable'] as const;

export const PIPELINE_BLOCKER_CODES = [
  'lookup_pending',
  'lookup_unavailable',
  'runs_absent',
  'commit_unverified',
  'runs_failed',
  'runs_active',
  'runs_inconclusive',
] as const;

export const REFUSAL_CODES = ['latest_tag_moved', 'preview_stale', 'release_in_progress'] as const;

export const TagRefusalCode = {
  TagExists: 'tag_exists',
  TagNotNewer: 'tag_not_newer',
  ReleaseExists: 'release_exists',
} as const;

export const TAG_REFUSAL_CODES: readonly string[] = Object.values(TagRefusalCode);

export const RECONCILIATION_CODE = 'release_manual_reconciliation_required';

export const PipelineStateSchema = z.enum(PIPELINE_STATES);

export const LookupSchema = z.enum(LOOKUPS);

export const PipelineBlockerCodeSchema: z.ZodType<
  (typeof PIPELINE_BLOCKER_CODES)[number] | (string & {})
> = z.string();

const KnownPipelineStateSchema = PipelineStateSchema.catch(PipelineStateSchema.enum.unknown);

const KnownLookupSchema = LookupSchema.catch(LookupSchema.enum.unavailable);

export const PipelineBlockerSchema = z.object({
  code: PipelineBlockerCodeSchema,
  detail: z.string(),
});

export const ReleaseIdentitySchema = z.object({
  repository: z.string(),
  id: z.number().int(),
  tag: z.string(),
  published_at: z.string(),
  url: WebUrlSchema,
});

export const PipelineRunSchema = z.object({
  id: z.number().int(),
  workflow_id: z.number().int(),
  attempt: z.number().int(),
  name: z.string(),
  path: z.string(),
  state: KnownPipelineStateSchema,
  head_sha: z.string(),
  created_at: z.string(),
  started_at: z.string().nullish(),
  updated_at: z.string(),
  url: WebUrlSchema,
});

export const ReleasePipelineSchema = z.object({
  release: ReleaseIdentitySchema,
  lookup: KnownLookupSchema,
  runs: z.array(PipelineRunSchema),
  checked_at: z.string(),
});

export const PipelineVerdictSchema = z.object({
  state: KnownPipelineStateSchema,
  succeeded: z.boolean(),
  complete: z.boolean(),
  blockers: z.array(PipelineBlockerSchema).default([]),
});

export const IncludedPullSchema = z.object({
  number: z.number().int(),
  url: WebUrlSchema,
  title: z.string(),
});

export const ReleaseSchema = PipelineVerdictSchema.extend({
  source_id: z.string(),
  repository: z.string(),
  repository_url: WebUrlSchema,
  id: z.number().int(),
  tag: z.string(),
  name: z.string().nullable(),
  url: WebUrlSchema,
  published_at: z.string(),
  prerelease: z.boolean(),
  pulls: z.array(IncludedPullSchema).default([]),
  pipeline: ReleasePipelineSchema,
});

export const ReleasesResponseSchema = z.object({
  generated_at: z.string(),
  stale: z.boolean(),
  partial: z.boolean(),
  window_days: z.number().int(),
  ttl_secs: z.number().int().nonnegative(),
  sources: z.array(SourceStatusSchema),
  releases: z.array(ReleaseSchema),
  warnings: z.array(z.string()).default([]),
});

export const TagLineSchema = z.object({
  prefix: z.string().nullable(),
  latest_tag: z.string().nullable(),
  next_tag: z.string(),
  previous_tags: z.array(z.string()).default([]),
});

export const SourceOptionsSchema = z.object({
  source_id: z.string(),
  repository: z.string(),
  repository_url: WebUrlSchema,
  default_branch: z.string(),
  latest_tag: z.string().nullable(),
  next_tag: z.string(),
  previous_tags: z.array(z.string()).default([]),
  lines: z.array(TagLineSchema).default([]),
});

export const OptionsResponseSchema = z.object({
  generated_at: z.string(),
  sources: z.array(SourceOptionsSchema),
  warnings: z.array(z.string()).default([]),
});

export const PipelineResponseSchema = PipelineVerdictSchema.extend({
  pipeline: ReleasePipelineSchema,
});

export const PreviewSchema = z.object({
  source_id: z.string(),
  repository: z.string(),
  tag: z.string(),
  name: z.string(),
  body: z.string(),
  base_tag: z.string().nullable(),
  target: z.string(),
  prerelease: z.boolean(),
  pulls: z.array(IncludedPullSchema).default([]),
  digest: z.string(),
});

export const PreviewResponseSchema = z.object({
  preview: PreviewSchema,
});

export const PreviewRequestSchema = z.object({
  source_id: z.string().min(1),
  tag: z.string().trim().min(1, 'Tag is required'),
  expected_latest_tag: z.string().nullable(),
});

export const CreateReleaseRequestSchema = PreviewRequestSchema.extend({
  prerelease: z.boolean(),
  digest: z.string().min(1),
});

export const CreatedSchema = z.object({
  source_id: z.string(),
  id: z.number().int(),
  tag: z.string(),
  name: z.string().nullable(),
  url: WebUrlSchema,
  published_at: z.string(),
});

export const CreatedResponseSchema = z.object({
  release: CreatedSchema,
});

export const RefusalSchema = z.discriminatedUnion('code', [
  z.object({
    error: z.string(),
    code: z.enum(REFUSAL_CODES),
  }),
  z.object({
    error: z.string(),
    code: z.literal(TagRefusalCode.TagExists),
    tag: z.string(),
    tag_object: z.string().optional(),
  }),
  z.object({
    error: z.string(),
    code: z.literal(TagRefusalCode.TagNotNewer),
    tag: z.string(),
    latest_tag: z.string(),
  }),
  z.object({
    error: z.string(),
    code: z.literal(TagRefusalCode.ReleaseExists),
    tag: z.string(),
    release_id: z.number().int(),
  }),
  z.object({
    error: z.string(),
    code: z.literal(RECONCILIATION_CODE),
    tag: z.string(),
    tag_object: z.string(),
    reference_created: z.boolean().nullable(),
    release_id: z.number().int().nullable(),
    release_exists: z.boolean(),
    marker: z.string(),
  }),
]);
