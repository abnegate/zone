import { describe, expect, it } from 'bun:test';
import { wireOverview } from '../../test/analytics';
import {
  ErrorCategorySchema,
  OverviewSchema,
  TimePeriodSchema,
  TrendDirectionSchema,
} from './schemas';

function without(record: Record<string, unknown>, key: string): Record<string, unknown> {
  const { [key]: _, ...rest } = record;
  return rest;
}

describe('analytics overview schema', () => {
  it('reads the overview the server sends', () => {
    const overview = OverviewSchema.parse(wireOverview);
    expect(overview.period).toBe('week');
    expect(overview.runs.outcomes.categories[0].category).toBe('test_failure');
    expect(overview.sources[1].source_id).toBeNull();
    expect(overview.models[1].model).toBeNull();
    expect(overview.tasks.statuses).toHaveLength(8);
  });

  it('requires every top level key', () => {
    for (const key of Object.keys(wireOverview)) {
      expect(OverviewSchema.safeParse(without(wireOverview, key)).success).toBe(false);
    }
  });

  it('requires every pull request key, even the nullable ones', () => {
    for (const key of Object.keys(wireOverview.pull_requests)) {
      const pullRequests = without(wireOverview.pull_requests, key);
      expect(
        OverviewSchema.safeParse({ ...wireOverview, pull_requests: pullRequests }).success
      ).toBe(false);
    }
  });

  it('accepts null for every pull request figure with nothing merged to measure', () => {
    const overview = OverviewSchema.parse({
      ...wireOverview,
      pull_requests: {
        ...wireOverview.pull_requests,
        merge_rate: null,
        median_minutes_to_merge: null,
        mean_review_cycles: null,
        mean_approvals: null,
      },
    });
    expect(overview.pull_requests.merge_rate).toBeNull();
    expect(overview.pull_requests.mean_approvals).toBeNull();
  });

  it('accepts a trend with neither half measured', () => {
    const runs = {
      ...wireOverview.runs,
      trend: {
        direction: 'steady',
        earlier_success_rate: null,
        later_success_rate: null,
        change: 0,
      },
    };
    expect(
      OverviewSchema.parse({ ...wireOverview, runs }).runs.trend.earlier_success_rate
    ).toBeNull();
  });

  it('refuses a period the server never sends', () => {
    expect(OverviewSchema.safeParse({ ...wireOverview, period: 'year' }).success).toBe(false);
  });

  it('reads a failure kind it does not know as unknown rather than dropping the page', () => {
    const outcomes = {
      ...wireOverview.runs.outcomes,
      categories: [{ ...wireOverview.runs.outcomes.categories[0], category: 'cosmic_ray' }],
    };
    const overview = OverviewSchema.parse({
      ...wireOverview,
      runs: { ...wireOverview.runs, outcomes },
    });
    expect(overview.runs.outcomes.categories[0].category).toBe('unknown');
  });

  it('refuses a negative count', () => {
    const evaluations = { ...wireOverview.evaluations, regressed: -1 };
    expect(OverviewSchema.safeParse({ ...wireOverview, evaluations }).success).toBe(false);
  });

  it('names the vocabularies the contract pins', () => {
    expect(TimePeriodSchema.options).toEqual(['day', 'week', 'month', 'quarter']);
    expect(TrendDirectionSchema.options).toEqual(['improving', 'steady', 'declining']);
    expect(ErrorCategorySchema.options).toContain('merge_conflict');
    expect(ErrorCategorySchema.options).toHaveLength(11);
  });
});
