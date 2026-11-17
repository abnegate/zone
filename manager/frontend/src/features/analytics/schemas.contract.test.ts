import { describe, expect, test } from 'bun:test';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { rustShape, zodShape } from '../../test/contract';
import { rustVariants } from '../../test/rust';
import { MAXIMUM_PULL_REQUESTS, MAXIMUM_RUNS } from './components/TruncationNotice';
import {
  AgentAnalyticsSchema,
  CategoryStatisticsSchema,
  CompletionTimesSchema,
  ErrorCategorySchema,
  EvaluationSummarySchema,
  ModelBreakdownSchema,
  OriginSchema,
  OutcomeStatisticsSchema,
  OverviewSchema,
  PullRequestSummarySchema,
  QualityBandSchema,
  SeriesPointSchema,
  SourceBreakdownSchema,
  TallySchema,
  TaskSummarySchema,
  TimePeriodSchema,
  TimeWindowSchema,
  TrendDirectionSchema,
  TrendSchema,
  TruncationSchema,
} from './schemas';

const SERVER = join(import.meta.dir, '../../../../../runner/zone_server/src');
const SERVICE = join(SERVER, 'services/analytics');
const WORKER = join(SERVER, 'workers/analytics');
const LEARNING = join(SERVER, 'workers/learning');
const ANALYTICS = join(SERVER, 'services/analytics.rs');
const ORIGIN = join(SERVER, 'db/tasks/origin.rs');

const read = (path: string): string => readFileSync(path, 'utf8');

type Shaped = z.ZodObject<z.ZodRawShape>;

function sources(directory: string): string[] {
  if (!existsSync(directory)) throw new Error(`${directory} does not exist`);
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sources(path);
    return entry.name.endsWith('.rs') ? [read(path)] : [];
  });
}

function declaring(directory: string, structName: string): string {
  const declaration = new RegExp(`\\bstruct ${structName}\\s*\\{`);
  const source = sources(directory).find((text) => declaration.test(text));
  if (!source) throw new Error(`${structName} is not declared under ${directory}`);
  return source;
}

function rustConstant(path: string, name: string): number {
  const value = read(path).match(new RegExp(`pub const ${name}: i64 = ([0-9_]+);`))?.[1];
  if (value === undefined) throw new Error(`${name} is not an integer constant in ${path}`);
  return Number(value.replaceAll('_', ''));
}

const variants = (path: string, enumName: string): string[] =>
  rustVariants(read(path), enumName).sort();

const sorted = (values: readonly string[]): string[] => [...values].sort();

describe('the analytics overview reads as the service serialises it', () => {
  test.each<[string, Shaped]>([
    ['Overview', OverviewSchema],
    ['Truncation', TruncationSchema],
    ['SourceBreakdown', SourceBreakdownSchema],
    ['ModelBreakdown', ModelBreakdownSchema],
    ['TaskSummary', TaskSummarySchema],
    ['PullRequestSummary', PullRequestSummarySchema],
    ['EvaluationSummary', EvaluationSummarySchema],
    ['Tally', TallySchema],
  ])(
    '%s carries the same fields on both sides, null where Rust sends None',
    (structName, schema) => {
      expect(zodShape(schema)).toEqual(rustShape(declaring(SERVICE, structName), structName));
    }
  );

  test('the caps the notice names are the ones the service reads under', () => {
    expect(MAXIMUM_RUNS).toBe(rustConstant(ANALYTICS, 'MAXIMUM_RUNS'));
    expect(MAXIMUM_PULL_REQUESTS).toBe(rustConstant(ANALYTICS, 'MAXIMUM_PULL_REQUESTS'));
  });
});

describe('the run analytics read as the worker serialises them', () => {
  test.each<[string, string, Shaped]>([
    ['AgentAnalytics', join(WORKER, 'summary.rs'), AgentAnalyticsSchema],
    ['CompletionTimes', join(WORKER, 'summary.rs'), CompletionTimesSchema],
    ['SeriesPoint', join(WORKER, 'summary.rs'), SeriesPointSchema],
    ['Trend', join(WORKER, 'summary.rs'), TrendSchema],
    ['TimeWindow', join(WORKER, 'period.rs'), TimeWindowSchema],
    ['OutcomeStatistics', join(LEARNING, 'attempt.rs'), OutcomeStatisticsSchema],
    ['CategoryStatistics', join(LEARNING, 'attempt.rs'), CategoryStatisticsSchema],
  ])(
    '%s carries the same fields on both sides, null where Rust sends None',
    (structName, path, schema) => {
      expect(zodShape(schema)).toEqual(rustShape(read(path), structName));
    }
  );
});

describe('the analytics vocabularies match the server', () => {
  test('time periods', () => {
    expect(sorted(TimePeriodSchema.options)).toEqual(
      variants(join(WORKER, 'period.rs'), 'TimePeriod')
    );
  });

  test('trend directions', () => {
    expect(sorted(TrendDirectionSchema.options)).toEqual(
      variants(join(WORKER, 'summary.rs'), 'TrendDirection')
    );
  });

  test('error categories', () => {
    expect(sorted(ErrorCategorySchema.options)).toEqual(
      variants(join(LEARNING, 'error_category.rs'), 'ErrorCategory')
    );
  });

  test('task origins', () => {
    expect(sorted(OriginSchema.options)).toEqual(variants(ORIGIN, 'Origin'));
  });

  test('pull request quality bands', () => {
    expect(sorted(QualityBandSchema.options)).toEqual(
      variants(join(LEARNING, 'quality.rs'), 'QualityBand')
    );
  });
});
