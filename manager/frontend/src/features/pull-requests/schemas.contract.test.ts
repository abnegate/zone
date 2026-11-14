import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { rustVariants, structFields } from '../../test/rust';
import { FreshnessSchema, SourceStatusSchema } from '../github';
import {
  BLOCKER_CODES,
  BlockerSchema,
  BotCommentSchema,
  CheckCountsSchema,
  CheckDetailSchema,
  CheckKindSchema,
  CheckLogResponseSchema,
  CheckLogSchema,
  CheckStateSchema,
  CheckSummarySchema,
  CommitListSchema,
  CommitSchema,
  CommitsResponseSchema,
  ConfidenceSchema,
  DiffFileSchema,
  DiffLineSchema,
  DiffResponseSchema,
  DiffSchema,
  FileStatusSchema,
  HunkSchema,
  LineKindSchema,
  MergedResponseSchema,
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

const SERVER = join(import.meta.dir, '../../../../../runner/zone_server/src');
const GITHUB = join(SERVER, 'services/github');
const PULL_REQUESTS = join(GITHUB, 'pull_requests');
const ROUTES = join(SERVER, 'routes/pull_requests');

const read = (path: string): string => readFileSync(path, 'utf8');

type Shaped = z.ZodObject<z.ZodRawShape>;

function rustShape(path: string, structName: string): Record<string, boolean> {
  return Object.fromEntries(
    structFields(read(path), structName).map((field) => [field.name, field.optional])
  );
}

/// Whether the parsed value may lack the key. `isOptional()` would also count a
/// `.default()` or `.catch()`, which only make the console tolerant of a field
/// the server always sends; serde omits a key only under `skip_serializing_if`,
/// and that is what the output optionality must mirror.
function zodShape(schema: Shaped): Record<string, boolean> {
  return Object.fromEntries(
    Object.entries(schema.shape).map(([key, value]) => [key, value._zod.optout === 'optional'])
  );
}

function rustConstants(path: string): string[] {
  return [...read(path).matchAll(/pub const [A-Z_]+: &str = "([^"]+)";/g)]
    .map((match) => match[1])
    .sort();
}

const variants = (path: string, enumName: string): string[] =>
  rustVariants(read(path), enumName)
    .filter((variant) => !variant.startsWith('#'))
    .sort();

const sorted = (values: readonly string[]): string[] => [...values].sort();

describe('a pull request reads as the server serialises it', () => {
  test.each<[string, string, string, Shaped]>([
    ['PullRequest', PULL_REQUESTS, 'pull_request.rs', PullRequestSchema],
    ['Detail', PULL_REQUESTS, 'detail.rs', PullRequestDetailSchema],
    ['Thread', PULL_REQUESTS, 'thread.rs', ThreadSchema],
    ['ThreadComment', PULL_REQUESTS, 'thread_comment.rs', ThreadCommentSchema],
    ['BotComment', PULL_REQUESTS, 'bot_comment.rs', BotCommentSchema],
    ['Commit', PULL_REQUESTS, 'commit.rs', CommitSchema],
    ['Commits', PULL_REQUESTS, 'commits.rs', CommitListSchema],
    ['Log', PULL_REQUESTS, 'log.rs', CheckLogSchema],
    ['Merged', PULL_REQUESTS, 'merged.rs', MergedSchema],
    ['Counts', PULL_REQUESTS, 'counts.rs', ReadinessCountsSchema],
    ['CheckDetail', join(GITHUB, 'evidence'), 'check_detail.rs', CheckDetailSchema],
    ['SourceStatus', GITHUB, 'source_status.rs', SourceStatusSchema],
    ['CheckCounts', GITHUB, 'readiness.rs', CheckCountsSchema],
    ['CheckSummary', GITHUB, 'readiness.rs', CheckSummarySchema],
    ['Confidence', GITHUB, 'readiness.rs', ConfidenceSchema],
    ['ReviewSummary', GITHUB, 'readiness.rs', ReviewSummarySchema],
    ['BlockerReport', GITHUB, 'readiness.rs', BlockerSchema],
  ])('%s carries the same fields on both sides', (structName, directory, file, schema) => {
    expect(zodShape(schema)).toEqual(rustShape(join(directory, file), structName));
  });
});

describe('a diff reads as the server serialises it', () => {
  test.each<[string, string, Shaped]>([
    ['Diff', 'diff.rs', DiffSchema],
    ['File', 'diff/file.rs', DiffFileSchema],
    ['Hunk', 'diff/hunk.rs', HunkSchema],
    ['Line', 'diff/line.rs', DiffLineSchema],
  ])('%s carries the same fields on both sides', (structName, file, schema) => {
    expect(zodShape(schema)).toEqual(rustShape(join(GITHUB, file), structName));
  });
});

describe('the pull request routes answer in the envelopes the console reads', () => {
  test.each<[string, string, Shaped]>([
    ['ListingResponse', 'listing_response.rs', PullRequestsResponseSchema],
    ['DiffResponse', 'diff_response.rs', DiffResponseSchema],
    ['CommitsResponse', 'commits_response.rs', CommitsResponseSchema],
    ['LogResponse', 'log_response.rs', CheckLogResponseSchema],
    ['MergedResponse', 'merged_response.rs', MergedResponseSchema],
  ])('%s carries the same fields on both sides', (structName, file, schema) => {
    expect(zodShape(schema)).toEqual(rustShape(join(ROUTES, file), structName));
  });

  test('a refusal carries every field but the success flag, which is always false', () => {
    const rust = rustShape(join(ROUTES, 'refusal_response.rs'), 'RefusalResponse');
    expect(rust).toHaveProperty('success', false);
    const { success: _, ...refusal } = rust;

    expect(zodShape(RefusalSchema)).toEqual(refusal);
  });

  test('a merge sends the fields the server reads', () => {
    expect(Object.keys(MergeRequestSchema.shape).sort()).toEqual(
      Object.keys(rustShape(join(ROUTES, 'merge_request.rs'), 'MergeRequest')).sort()
    );
  });
});

describe('the pull request vocabularies match the server', () => {
  test('readiness', () => {
    expect(sorted(ReadinessSchema.options)).toEqual(
      variants(join(PULL_REQUESTS, 'readiness.rs'), 'Readiness')
    );
  });

  test('check states', () => {
    expect(sorted(CheckStateSchema.options)).toEqual(
      variants(join(GITHUB, 'readiness.rs'), 'CheckState')
    );
  });

  test('check kinds, plus the fallback for one the console does not know', () => {
    expect(sorted(CheckKindSchema.exclude(['unknown']).options)).toEqual(
      variants(join(GITHUB, 'evidence/check_kind.rs'), 'CheckKind')
    );
  });

  test('blocker codes', () => {
    expect(sorted(BLOCKER_CODES)).toEqual(variants(join(GITHUB, 'readiness.rs'), 'BlockerCode'));
  });

  test('freshness', () => {
    expect(sorted(FreshnessSchema.options)).toEqual(
      variants(join(GITHUB, 'freshness.rs'), 'Freshness')
    );
  });

  test('diff file statuses', () => {
    expect(sorted(FileStatusSchema.options)).toEqual(
      variants(join(GITHUB, 'diff/status.rs'), 'Status')
    );
  });

  test('diff line kinds', () => {
    expect(sorted(LineKindSchema.options)).toEqual(
      variants(join(GITHUB, 'diff/line_kind.rs'), 'LineKind')
    );
  });

  test('merge methods', () => {
    expect(sorted(MergeMethodSchema.options)).toEqual(
      variants(join(PULL_REQUESTS, 'method.rs'), 'Method')
    );
  });

  test('refusal codes', () => {
    expect(sorted(RefusalCodeSchema.options)).toEqual(
      rustConstants(join(PULL_REQUESTS, 'refusal.rs'))
    );
  });
});
