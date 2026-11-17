import { describe, expect, it } from 'bun:test';
import { DEFAULT_PERIOD, isPeriod, PERIODS, periodLabel, periodOf } from './period';

describe('analytics periods', () => {
  it('offers every period the server answers, shortest first', () => {
    expect(PERIODS).toEqual([
      { value: 'day', label: '24 hours' },
      { value: 'week', label: '7 days' },
      { value: 'month', label: '30 days' },
      { value: 'quarter', label: '90 days' },
    ]);
  });

  it('defaults to a week', () => {
    expect(DEFAULT_PERIOD).toBe('week');
    expect(periodLabel(DEFAULT_PERIOD)).toBe('7 days');
  });

  it('reads a period from a search param and falls back to a week', () => {
    expect(periodOf('day')).toBe('day');
    expect(periodOf('quarter')).toBe('quarter');
    expect(periodOf('year')).toBe('week');
    expect(periodOf('')).toBe('week');
    expect(periodOf(null)).toBe('week');
  });

  it('knows a period from anything else', () => {
    expect(isPeriod('month')).toBe(true);
    expect(isPeriod('Month')).toBe(false);
    expect(isPeriod(null)).toBe(false);
  });
});
