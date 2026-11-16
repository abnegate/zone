import { afterAll, beforeAll, describe, expect, it } from 'bun:test';

const WEST_OF_UTC = 'America/Los_Angeles';
const zone = process.env.TZ;

let RunSeriesAxis: typeof import('./RunSeriesAxis').RunSeriesAxis;

beforeAll(async () => {
  process.env.TZ = WEST_OF_UTC;
  ({ RunSeriesAxis } = await import('./RunSeriesAxis'));
});

afterAll(() => {
  if (zone === undefined) delete process.env.TZ;
  else process.env.TZ = zone;
});

const MIDNIGHT = { start: '2026-11-16T00:00:00', end: '2026-11-17T00:00:00' };
const FIVE_AM = { start: '2026-11-16T05:00:00', end: '2026-11-16T06:00:00' };

describe('RunSeriesAxis', () => {
  it('rounds the axis up to 1, 2 or 5 times a power of ten', () => {
    expect(RunSeriesAxis.ceiling(0)).toBe(1);
    expect(RunSeriesAxis.ceiling(1)).toBe(1);
    expect(RunSeriesAxis.ceiling(3)).toBe(5);
    expect(RunSeriesAxis.ceiling(9)).toBe(10);
    expect(RunSeriesAxis.ceiling(10)).toBe(10);
    expect(RunSeriesAxis.ceiling(11)).toBe(20);
    expect(RunSeriesAxis.ceiling(240)).toBe(500);
  });

  it('labels every point when they fit and thins them when they do not', () => {
    expect(RunSeriesAxis.labelEvery(7, 7)).toBe(1);
    expect(RunSeriesAxis.labelEvery(30, 7)).toBe(5);
    expect(RunSeriesAxis.labelEvery(24, 8)).toBe(3);
    expect(RunSeriesAxis.labelEvery(0, 7)).toBe(1);
  });

  it('labels a UTC day bucket by its UTC date west of UTC', () => {
    expect(process.env.TZ).toBe(WEST_OF_UTC);
    expect(RunSeriesAxis.tick(MIDNIGHT, 'week')).toBe('Nov 16');
    expect(RunSeriesAxis.bucket(MIDNIGHT, 'month')).toBe('Nov 16');
    expect(RunSeriesAxis.tick(MIDNIGHT, 'quarter')).toBe('Nov 16');
  });

  it('labels an hour bucket by its UTC hour and names the zone in full', () => {
    expect(RunSeriesAxis.tick(FIVE_AM, 'day')).toMatch(/^5\sAM$/);
    expect(RunSeriesAxis.bucket(FIVE_AM, 'day')).toMatch(/^Nov 16\b.*\b5\sAM UTC$/);
  });

  it('names the clock the buckets follow', () => {
    expect(RunSeriesAxis.note('day')).toBe('Hours in UTC');
    expect(RunSeriesAxis.note('week')).toBe('Days in UTC');
  });

  it('hands back a bucket start it cannot read unchanged', () => {
    expect(RunSeriesAxis.tick({ start: 'soon', end: 'later' }, 'week')).toBe('soon');
  });
});
