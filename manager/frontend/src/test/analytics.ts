import { OverviewSchema } from '../features/analytics/schemas';
import type { Overview, SeriesPoint } from '../features/analytics/types';

export const WINDOW = { start: '2026-11-09T00:00:00', end: '2026-11-16T00:00:00' };

const EMPTY_OUTCOMES = {
  total: 0,
  succeeded: 0,
  failed: 0,
  cancelled: 0,
  success_rate: 0,
  recent_success_rate: 0,
  categories: [],
};

const EMPTY_COMPLETION = {
  measured: 0,
  median_seconds: 0,
  ninetieth_seconds: 0,
  mean_seconds: 0,
};

function day(offset: number, total: number, failed: number, cancelled: number): SeriesPoint {
  return {
    window: {
      start: `2026-11-${String(9 + offset).padStart(2, '0')}T00:00:00`,
      end: `2026-11-${String(10 + offset).padStart(2, '0')}T00:00:00`,
    },
    total,
    succeeded: total - failed - cancelled,
    failed,
    cancelled,
  };
}

export const wireOverview = {
  period: 'week',
  window: WINDOW,
  measured_from: null,
  truncated: { runs: false, pull_requests: false },
  runs: {
    period: 'week',
    window: WINDOW,
    outcomes: {
      total: 40,
      succeeded: 30,
      failed: 8,
      cancelled: 2,
      success_rate: 0.75,
      recent_success_rate: 0.82,
      categories: [
        {
          category: 'test_failure',
          occurrences: 5,
          recent_weight: 4.2,
          distinct_tasks: 3,
          last_seen: '2026-11-15T18:30:00.123456',
        },
        {
          category: 'build',
          occurrences: 3,
          recent_weight: 1.5,
          distinct_tasks: 2,
          last_seen: '2026-11-14T09:00:00',
        },
      ],
    },
    completion: {
      measured: 38,
      median_seconds: 754,
      ninetieth_seconds: 3_840,
      mean_seconds: 1_200,
    },
    series: [
      day(0, 4, 1, 0),
      day(1, 6, 2, 0),
      day(2, 0, 0, 0),
      day(3, 8, 1, 1),
      day(4, 7, 2, 0),
      day(5, 9, 1, 1),
      day(6, 6, 1, 0),
    ],
    trend: {
      direction: 'improving',
      earlier_success_rate: 0.68,
      later_success_rate: 0.8,
      change: 0.12,
    },
  },
  sources: [
    {
      source_id: 'source-1',
      name: 'acme/zone',
      source_type: 'github',
      outcomes: {
        ...EMPTY_OUTCOMES,
        total: 28,
        succeeded: 22,
        failed: 5,
        cancelled: 1,
        success_rate: 22 / 28,
        recent_success_rate: 0.8,
      },
      merged_pull_requests: 9,
    },
    {
      source_id: null,
      name: null,
      source_type: null,
      outcomes: {
        ...EMPTY_OUTCOMES,
        total: 12,
        succeeded: 8,
        failed: 3,
        cancelled: 1,
        success_rate: 8 / 12,
        recent_success_rate: 0.7,
      },
      merged_pull_requests: 0,
    },
  ],
  models: [
    {
      model: 'claude-opus',
      outcomes: {
        ...EMPTY_OUTCOMES,
        total: 30,
        succeeded: 25,
        failed: 4,
        cancelled: 1,
        success_rate: 25 / 30,
        recent_success_rate: 0.85,
      },
      completion: {
        measured: 29,
        median_seconds: 600,
        ninetieth_seconds: 2_400,
        mean_seconds: 900,
      },
    },
    {
      model: null,
      outcomes: {
        ...EMPTY_OUTCOMES,
        total: 10,
        succeeded: 5,
        failed: 4,
        cancelled: 1,
        success_rate: 0.5,
        recent_success_rate: 0.5,
      },
      completion: EMPTY_COMPLETION,
    },
  ],
  tasks: {
    statuses: [
      { name: 'created', count: 4 },
      { name: 'queued', count: 2 },
      { name: 'in_progress', count: 3 },
      { name: 'blocked', count: 1 },
      { name: 'paused', count: 0 },
      { name: 'review', count: 5 },
      { name: 'complete', count: 18 },
      { name: 'cancelled', count: 2 },
    ],
    origins: [
      { name: 'member', count: 20 },
      { name: 'reminder', count: 3 },
      { name: 'automation', count: 12 },
    ],
    created: 35,
    shipped: 18,
    reopened: 2,
  },
  pull_requests: {
    opened: 14,
    merged: 9,
    closed: 2,
    open: 3,
    merge_rate: 9 / 11,
    median_minutes_to_merge: 95,
    mean_review_cycles: 1.6,
    mean_approvals: 1.2,
    quality: [
      { name: 'poor', count: 1 },
      { name: 'mixed', count: 2 },
      { name: 'solid', count: 5 },
      { name: 'exemplary', count: 1 },
    ],
  },
  evaluations: {
    improved: 6,
    unchanged: 8,
    regressed: 1,
    not_measured: 3,
  },
};

export function overview(changes: Partial<Overview> = {}): Overview {
  return { ...OverviewSchema.parse(wireOverview), ...changes };
}

export function emptyOverview(): Overview {
  const populated = overview();
  return {
    ...populated,
    runs: {
      ...populated.runs,
      outcomes: EMPTY_OUTCOMES,
      completion: EMPTY_COMPLETION,
      series: populated.runs.series.map((point) => ({
        ...point,
        total: 0,
        succeeded: 0,
        failed: 0,
        cancelled: 0,
      })),
      trend: {
        direction: 'steady',
        earlier_success_rate: null,
        later_success_rate: null,
        change: 0,
      },
    },
    sources: [],
    models: [],
  };
}
