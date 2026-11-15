import type { z } from 'zod';
import type {
  BlockerCodeSchema,
  BlockerSchema,
  BotCommentSchema,
  CheckCountsSchema,
  CheckDetailSchema,
  CheckKindSchema,
  CheckLogSchema,
  CheckStateSchema,
  CheckSummarySchema,
  CommitListSchema,
  CommitSchema,
  ConfidenceSchema,
  DiffFileSchema,
  DiffLineSchema,
  DiffSchema,
  FileStatusSchema,
  HunkSchema,
  LineKindSchema,
  MergedSchema,
  MergeMethodSchema,
  MergeRequestSchema,
  PullRequestDetailSchema,
  PullRequestSchema,
  PullRequestsResponseSchema,
  ReadinessCountsSchema,
  ReadinessSchema,
  RefusalCodeSchema,
  RefusalSchema,
  ReviewSummarySchema,
  ThreadCommentSchema,
  ThreadSchema,
} from './schemas';

export type Readiness = z.infer<typeof ReadinessSchema>;
export type CheckState = z.infer<typeof CheckStateSchema>;
export type CheckKind = z.infer<typeof CheckKindSchema>;
export type FileStatus = z.infer<typeof FileStatusSchema>;
export type LineKind = z.infer<typeof LineKindSchema>;
export type MergeMethod = z.infer<typeof MergeMethodSchema>;
export type RefusalCode = z.infer<typeof RefusalCodeSchema>;
export type BlockerCode = z.infer<typeof BlockerCodeSchema>;
export type Blocker = z.infer<typeof BlockerSchema>;
export type CheckCounts = z.infer<typeof CheckCountsSchema>;
export type CheckSummary = z.infer<typeof CheckSummarySchema>;
export type CheckDetail = z.infer<typeof CheckDetailSchema>;
export type Confidence = z.infer<typeof ConfidenceSchema>;
export type ReviewSummary = z.infer<typeof ReviewSummarySchema>;
export type PullRequest = z.infer<typeof PullRequestSchema>;
export type PullRequestReference = Pick<PullRequest, 'source_id' | 'number'>;
export type PullRequestRevision = Pick<PullRequest, 'source_id' | 'number' | 'head'>;
export type ReadinessCounts = z.infer<typeof ReadinessCountsSchema>;
export type PullRequestsResponse = z.infer<typeof PullRequestsResponseSchema>;
export type ThreadComment = z.infer<typeof ThreadCommentSchema>;
export type Thread = z.infer<typeof ThreadSchema>;
export type BotComment = z.infer<typeof BotCommentSchema>;
export type PullRequestDetail = z.infer<typeof PullRequestDetailSchema>;
export type DiffLine = z.infer<typeof DiffLineSchema>;
export type Hunk = z.infer<typeof HunkSchema>;
export type DiffFile = z.infer<typeof DiffFileSchema>;
export type Diff = z.infer<typeof DiffSchema>;
export type Commit = z.infer<typeof CommitSchema>;
export type CommitList = z.infer<typeof CommitListSchema>;
export type CheckLog = z.infer<typeof CheckLogSchema>;
export type MergeRequest = z.infer<typeof MergeRequestSchema>;
export type Merged = z.infer<typeof MergedSchema>;
export type Refusal = z.infer<typeof RefusalSchema>;
