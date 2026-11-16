import { describe, expect, it } from 'bun:test';
import {
  bucketLabel,
  count,
  decimal,
  duration,
  measured,
  minutes,
  NOT_MEASURED,
  points,
  share,
  utc,
} from './format';

describe('analytics formatting', () => {
  it('reads a naive timestamp as UTC', () => {
    expect(utc('2026-11-16T10:00:00').toISOString()).toBe('2026-11-16T10:00:00.000Z');
  });

  it('reads a naive timestamp with microseconds', () => {
    expect(utc('2026-11-15T18:30:00.123456').toISOString()).toBe('2026-11-15T18:30:00.123Z');
  });

  it('formats durations from seconds to days', () => {
    expect(duration(42)).toBe('42s');
    expect(duration(754)).toBe('13m');
    expect(duration(3_600)).toBe('1h');
    expect(duration(3_840)).toBe('1h 4m');
    expect(duration(90_000)).toBe('1d 1h');
    expect(duration(172_800)).toBe('2d');
  });

  it('shows nothing measured rather than a zero duration', () => {
    expect(measured(0, 0)).toBe(NOT_MEASURED);
    expect(measured(754, 3)).toBe('13m');
    expect(minutes(null)).toBe(NOT_MEASURED);
    expect(minutes(95)).toBe('1h 35m');
  });

  it('formats shares as whole percentages and nulls as not measured', () => {
    expect(share(0.75)).toBe('75%');
    expect(share(null)).toBe(NOT_MEASURED);
  });

  it('formats a change in points', () => {
    expect(points(0.12)).toBe('12 points');
    expect(points(-0.01)).toBe('1 point');
  });

  it('formats counts and decimals', () => {
    expect(count(1_234)).toBe('1,234');
    expect(decimal(1.64)).toBe('1.6');
    expect(decimal(null)).toBe(NOT_MEASURED);
  });

  it('labels a bucket by day, or by hour over a day', () => {
    const window = { start: '2026-11-12T00:00:00', end: '2026-11-13T00:00:00' };
    expect(bucketLabel(window, 'week')).toMatch(/Nov/);
    expect(bucketLabel(window, 'day')).not.toMatch(/Nov/);
  });
});
