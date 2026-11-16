import { describe, expect, it } from 'bun:test';
import { CheckStateSchema } from '../schemas';
import type { CheckCounts, CheckSummary } from '../types';
import { CHECK_LABELS, CHECK_TINTS, cardBadge, summarizeChecks } from './status';

const NONE: CheckCounts = {
  total: 0,
  passed: 0,
  failed: 0,
  running: 0,
  queued: 0,
  in_progress: 0,
  unknown: 0,
};

function summary(counts: Partial<CheckCounts>, complete = true): CheckSummary {
  return { state: 'pending', complete, counts: { ...NONE, ...counts } };
}

describe('check tints and labels', () => {
  it('covers every check state', () => {
    for (const state of CheckStateSchema.options) {
      expect(CHECK_TINTS[state]).toBeDefined();
      expect(CHECK_LABELS[state]).toBeDefined();
    }
  });

  it('tints passing, failing and running checks apart', () => {
    expect(CHECK_TINTS.success).toBe('success');
    expect(CHECK_TINTS.failure).toBe('destructive');
    expect(CHECK_TINTS.pending).toBe('info');
    expect(CHECK_TINTS.none).toBe('neutral');
    expect(CHECK_TINTS.unknown).toBe('neutral');
  });
});

describe('summarizeChecks', () => {
  it('says there are no checks', () => {
    expect(summarizeChecks(summary({}))).toBe('No checks');
  });

  it('counts the checks that passed', () => {
    expect(summarizeChecks(summary({ total: 3, passed: 3 }))).toBe('3/3 passed');
  });

  it('leads with the checks that failed', () => {
    expect(summarizeChecks(summary({ total: 3, passed: 2, failed: 1 }))).toBe(
      '1 failed · 2/3 passed'
    );
  });

  it('counts running, queued and in-progress checks as running', () => {
    expect(
      summarizeChecks(summary({ total: 5, passed: 1, running: 1, queued: 2, in_progress: 1 }))
    ).toBe('4 running · 1/5 passed');
  });

  it('says when GitHub did not report every check', () => {
    expect(summarizeChecks(summary({ total: 2, passed: 1, failed: 1 }, false))).toBe(
      '1 failed · 1/2 passed · incomplete'
    );
  });
});

describe('cardBadge', () => {
  it('shows the readiness of an open pull request', () => {
    expect(cardBadge({ draft: false, readiness: 'ready' })).toEqual({
      label: 'Ready',
      tint: 'success',
    });
    expect(cardBadge({ draft: false, readiness: 'not_ready' })).toEqual({
      label: 'Not ready',
      tint: 'warning',
    });
  });

  it('shows a draft as a draft whatever its readiness', () => {
    expect(cardBadge({ draft: true, readiness: 'not_ready' })).toEqual({
      label: 'Draft',
      tint: 'neutral',
    });
  });
});
