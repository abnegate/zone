import { z } from 'zod';
import { SourceStatusSchema, WebUrlSchema } from '../github';

export const BLOCKER_CODES = [
  'draft',
  'head_commit_unverified',
  'threads_incomplete',
  'comments_incomplete',
  'unresolved_threads',
  'review_missing',
  'review_link_missing',
  'confidence_unreadable',
  'confidence_below_requirement',
  'reviewed_commit_unreadable',
  'reviewed_commit_stale',
  'checks_failed',
  'checks_pending',
  'checks_absent',
  'checks_incomplete',
] as const;

export const BlockerCodeSchema: z.ZodType<(typeof BLOCKER_CODES)[number] | (string & {})> =
  z.string();

export const ReadinessSchema = z.enum(['ready', 'in_progress', 'not_ready']);

export const CheckStateSchema = z.enum(['success', 'failure', 'pending', 'none', 'unknown']);

export const CheckKindSchema = z.enum(['workflow', 'check_run', 'status', 'unknown']);

export const FileStatusSchema = z.enum([
  'added',
  'removed',
  'modified',
  'renamed',
  'copied',
  'changed',
  'unchanged',
  'unknown',
]);

export const LineKindSchema = z.enum(['addition', 'deletion', 'context', 'meta']);

export const MergeMethodSchema = z.enum(['merge', 'squash', 'rebase']);

export const RefusalCodeSchema = z.enum([
  'head_moved',
  'not_ready',
  'conflict',
  'protected',
  'merged',
  'closed',
]);

const KnownReadinessSchema = ReadinessSchema.catch(ReadinessSchema.enum.not_ready);

const KnownCheckStateSchema = CheckStateSchema.catch(CheckStateSchema.enum.unknown);

const KnownCheckKindSchema = CheckKindSchema.catch(CheckKindSchema.enum.unknown);

const KnownFileStatusSchema = FileStatusSchema.catch(FileStatusSchema.enum.unknown);

const KnownLineKindSchema = LineKindSchema.catch(LineKindSchema.enum.meta);

const IdentifierSchema = z.union([z.number(), z.string()]);

export const BlockerSchema = z.object({
  code: BlockerCodeSchema,
  detail: z.string(),
});

export const CheckCountsSchema = z.object({
  total: z.number().int().nonnegative(),
  passed: z.number().int().nonnegative(),
  failed: z.number().int().nonnegative(),
  running: z.number().int().nonnegative(),
  queued: z.number().int().nonnegative(),
  in_progress: z.number().int().nonnegative(),
  unknown: z.number().int().nonnegative(),
});

export const CheckSummarySchema = z.object({
  state: KnownCheckStateSchema,
  complete: z.boolean(),
  counts: CheckCountsSchema,
});

export const CheckDetailSchema = z.object({
  kind: KnownCheckKindSchema,
  id: z.number().int().nullable(),
  name: z.string(),
  state: KnownCheckStateSchema,
  url: WebUrlSchema.nullable(),
  updated_at: z.string().nullable(),
});

export const ConfidenceSchema = z.object({
  score: z.number(),
  scale: z.number(),
});

export const ReviewSummarySchema = z.object({
  reviewer: z.string(),
  comment_id: z.number().int(),
  url: WebUrlSchema.nullable(),
  confidence: ConfidenceSchema.nullable(),
  reviewed_commit: z.string().nullable(),
  created_at: z.string(),
});

export const PullRequestSchema = z.object({
  source_id: z.string(),
  repository: z.string(),
  repository_url: WebUrlSchema,
  number: z.number().int().positive(),
  title: z.string(),
  url: WebUrlSchema,
  author: z.string().nullable(),
  updated_at: z.string(),
  draft: z.boolean(),
  head: z.string().nullable(),
  head_ref: z.string(),
  base_ref: z.string(),
  readiness: KnownReadinessSchema,
  blockers: z.array(BlockerSchema),
  checks: CheckSummarySchema,
  check_details: z.array(CheckDetailSchema),
  reviews: z.array(ReviewSummarySchema),
  review_current: z.boolean(),
  unresolved_threads: z.number().int().nonnegative(),
  threads_complete: z.boolean(),
  comments_complete: z.boolean(),
  assessed_at: z.string(),
  rank: z.number().int(),
});

export const ReadinessCountsSchema = z.object({
  ready: z.number().int().nonnegative(),
  in_progress: z.number().int().nonnegative(),
  not_ready: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
});

export const PullRequestsResponseSchema = z.object({
  generated_at: z.string(),
  stale: z.boolean(),
  partial: z.boolean(),
  ttl_secs: z.number().int().nonnegative(),
  refresh_floor_secs: z.number().nonnegative(),
  counts: ReadinessCountsSchema,
  sources: z.array(SourceStatusSchema),
  pull_requests: z.array(PullRequestSchema),
  warnings: z.array(z.string()).default([]),
});

export const ThreadCommentSchema = z.object({
  id: IdentifierSchema.nullable(),
  author: z.string(),
  body: z.string(),
  url: WebUrlSchema,
  created_at: z.string(),
});

export const ThreadSchema = z.object({
  id: z.string(),
  path: z.string().nullable(),
  line: z.number().int().nullable(),
  outdated: z.boolean(),
  url: WebUrlSchema.nullable(),
  comments: z.array(ThreadCommentSchema),
});

export const BotCommentSchema = z.object({
  reviewer: z.string(),
  body: z.string(),
  url: WebUrlSchema,
  created_at: z.string(),
});

export const PullRequestDetailSchema = z.object({
  pull_request: PullRequestSchema,
  threads: z.array(ThreadSchema),
  comments: z.array(BotCommentSchema),
});

export const DiffLineSchema = z.object({
  kind: KnownLineKindSchema,
  content: z.string(),
  old_line: z.number().int().nullable(),
  new_line: z.number().int().nullable(),
});

export const HunkSchema = z.object({
  header: z.string(),
  old_start: z.number().int().nonnegative(),
  old_lines: z.number().int().nonnegative(),
  new_start: z.number().int().nonnegative(),
  new_lines: z.number().int().nonnegative(),
  lines: z.array(DiffLineSchema),
});

export const DiffFileSchema = z.object({
  path: z.string(),
  previous_path: z.string().nullable(),
  status: KnownFileStatusSchema,
  additions: z.number().int().nonnegative(),
  deletions: z.number().int().nonnegative(),
  changes: z.number().int().nonnegative(),
  binary: z.boolean(),
  truncated: z.boolean(),
  blob_url: WebUrlSchema,
  hunks: z.array(HunkSchema),
});

export const DiffSchema = z.object({
  number: z.number().int().positive(),
  head: z.string(),
  base: z.string(),
  complete: z.boolean(),
  warning: z.string().nullable(),
  files: z.array(DiffFileSchema),
});

export const DiffResponseSchema = z.object({
  diff: DiffSchema,
});

export const CommitSchema = z.object({
  sha: z.string(),
  message: z.string(),
  author_name: z.string().nullable(),
  author_login: z.string().nullable(),
  authored_at: z.string().nullable(),
  url: WebUrlSchema,
});

export const CommitListSchema = z.object({
  number: z.number().int().positive(),
  head: z.string(),
  base: z.string(),
  complete: z.boolean(),
  commits: z.array(CommitSchema),
});

export const CommitsResponseSchema = z.object({
  commits: CommitListSchema,
});

export const CheckLogSchema = z.object({
  job_id: z.number().int(),
  run_id: z.number().int().nullable(),
  head: z.string(),
  text: z.string(),
  truncated: z.boolean(),
  fetched_at: z.string(),
});

export const CheckLogResponseSchema = z.object({
  log: CheckLogSchema,
});

export const MergeRequestSchema = z.object({
  expected_head: z.string().min(1),
  method: MergeMethodSchema.optional(),
});

export const MergedSchema = z.object({
  number: z.number().int().positive(),
  sha: z.string().nullable(),
  admin: z.boolean(),
  url: WebUrlSchema,
});

export const MergedResponseSchema = z.object({
  merged: MergedSchema,
});

export const RefusalSchema = z.object({
  error: z.string(),
  code: RefusalCodeSchema,
  current_head: z.string().nullish(),
  blockers: z.array(BlockerSchema).optional(),
});
