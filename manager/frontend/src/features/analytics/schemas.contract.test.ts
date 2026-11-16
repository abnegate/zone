import { describe, expect, test } from 'bun:test';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { rustVariants, structFields } from '../../test/rust';
import {
  AgentAnalyticsSchema,
  CategoryStatisticsSchema,
  CompletionTimesSchema,
  ErrorCategorySchema,
  EvaluationSummarySchema,
  ModelBreakdownSchema,
  OutcomeStatisticsSchema,
  OverviewSchema,
  PullRequestSummarySchema,
  SeriesPointSchema,
  SourceBreakdownSchema,
  TallySchema,
  TaskSummarySchema,
  TimePeriodSchema,
  TimeWindowSchema,
  TrendDirectionSchema,
  TrendSchema,
} from './schemas';

const SERVER = join(import.meta.dir, '../../../../../runner/zone_server/src');
const SERVICE = join(SERVER, 'services/analytics');
const WORKER = join(SERVER, 'workers/analytics');
const LEARNING = join(SERVER, 'workers/learning');

const read = (path: string): string => readFileSync(path, 'utf8');

type Shaped = z.ZodObject<z.ZodRawShape>;

function shapeOf(source: string, structName: string): Record<string, boolean> {
  return Object.fromEntries(
    structFields(source, structName).map((field) => [field.name, field.optional])
  );
}

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

function zodShape(schema: Shaped): Record<string, boolean> {
  return Object.fromEntries(
    Object.entries(schema.shape).map(([key, value]) => [key, value._zod.optout === 'optional'])
  );
}

const variants = (path: string, enumName: string): string[] =>
  rustVariants(read(path), enumName).sort();

const sorted = (values: readonly string[]): string[] => [...values].sort();

describe('the analytics overview reads as the service serialises it', () => {
  test.each<[string, Shaped]>([
    ['Overview', OverviewSchema],
    ['SourceBreakdown', SourceBreakdownSchema],
    ['ModelBreakdown', ModelBreakdownSchema],
    ['TaskSummary', TaskSummarySchema],
    ['PullRequestSummary', PullRequestSummarySchema],
    ['EvaluationSummary', EvaluationSummarySchema],
    ['Tally', TallySchema],
  ])('%s carries the same fields on both sides', (structName, schema) => {
    expect(zodShape(schema)).toEqual(shapeOf(declaring(SERVICE, structName), structName));
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
  ])('%s carries the same fields on both sides', (structName, path, schema) => {
    expect(zodShape(schema)).toEqual(shapeOf(read(path), structName));
  });
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
});
