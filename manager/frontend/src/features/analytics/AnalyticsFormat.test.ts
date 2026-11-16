import { afterAll, beforeAll, describe, expect, it } from 'bun:test';

const LOCAL_ZONE = 'America/Los_Angeles';
const previousZone = process.env.TZ;

let AnalyticsFormat: typeof import('./AnalyticsFormat').AnalyticsFormat;
let NOT_MEASURED: string;

beforeAll(async () => {
  process.env.TZ = LOCAL_ZONE;
  ({ AnalyticsFormat, NOT_MEASURED } = await import('./AnalyticsFormat'));
});

afterAll(() => {
  process.env.TZ = previousZone;
});

describe('AnalyticsFormat', () => {
  it('formats durations from seconds to days', () => {
    expect(AnalyticsFormat.duration(42)).toBe('42s');
    expect(AnalyticsFormat.duration(754)).toBe('13m');
    expect(AnalyticsFormat.duration(3_600)).toBe('1h');
    expect(AnalyticsFormat.duration(3_840)).toBe('1h 4m');
    expect(AnalyticsFormat.duration(90_000)).toBe('1d 1h');
    expect(AnalyticsFormat.duration(172_800)).toBe('2d');
  });

  it('shows nothing measured rather than a zero duration', () => {
    expect(AnalyticsFormat.measured(0, 0)).toBe(NOT_MEASURED);
    expect(AnalyticsFormat.measured(754, 3)).toBe('13m');
    expect(AnalyticsFormat.minutes(null)).toBe(NOT_MEASURED);
    expect(AnalyticsFormat.minutes(95)).toBe('1h 35m');
  });

  it('formats shares as whole percentages and nulls as not measured', () => {
    expect(AnalyticsFormat.share(0.75)).toBe('75%');
    expect(AnalyticsFormat.share(null)).toBe(NOT_MEASURED);
  });

  it('formats a change in points', () => {
    expect(AnalyticsFormat.points(0.12)).toBe('12 points');
    expect(AnalyticsFormat.points(-0.01)).toBe('1 point');
  });

  it('formats counts and decimals', () => {
    expect(AnalyticsFormat.count(1_234)).toBe('1,234');
    expect(AnalyticsFormat.decimal(1.64)).toBe('1.6');
    expect(AnalyticsFormat.decimal(null)).toBe(NOT_MEASURED);
  });

  it('dates a zoneless server timestamp on the UTC calendar the chart buckets use', () => {
    expect(new Date('2026-11-16T00:30:00Z').getDate()).toBe(15);
    expect(AnalyticsFormat.date('2026-11-15T23:30:00.123456')).toBe('Nov 15');
    expect(AnalyticsFormat.date('2026-11-16T00:30:00')).toBe('Nov 16');
  });

  it('hands back a timestamp it cannot read unchanged', () => {
    expect(AnalyticsFormat.date('yesterday')).toBe('yesterday');
  });
});
