import { beforeAll, describe, expect, it } from 'bun:test';
import { pinTimeZone } from '../../../test/timeZone';

const WEST_OF_UTC = 'America/Los_Angeles';

let RunSeriesAxis: typeof import('./RunSeriesAxis').RunSeriesAxis;
let TickRank: typeof import('./RunSeriesAxis').TickRank;

pinTimeZone(WEST_OF_UTC);

beforeAll(async () => {
  ({ RunSeriesAxis, TickRank } = await import('./RunSeriesAxis'));
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

  it('marks the axis only at whole numbers of runs', () => {
    expect(RunSeriesAxis.gridlines(1)).toEqual([1, 0]);
    expect(RunSeriesAxis.gridlines(2)).toEqual([2, 1, 0]);
    expect(RunSeriesAxis.gridlines(5)).toEqual([5, 0]);
    expect(RunSeriesAxis.gridlines(10)).toEqual([10, 5, 0]);
    expect(RunSeriesAxis.gridlines(50)).toEqual([50, 25, 0]);
    for (const ceiling of [1, 2, 5, 10, 20, 50, 100, 500]) {
      expect(RunSeriesAxis.gridlines(ceiling).every(Number.isInteger)).toBe(true);
    }
  });

  it('ranks every other labelled tick minor, whatever the label spacing', () => {
    const ranks = (points: number, every: number) =>
      Array.from({ length: points }, (_, index) => RunSeriesAxis.rank(index, every));

    expect(ranks(5, 1)).toEqual([
      TickRank.Major,
      TickRank.Minor,
      TickRank.Major,
      TickRank.Minor,
      TickRank.Major,
    ]);
    expect(ranks(9, 4)).toEqual([
      TickRank.Major,
      undefined,
      undefined,
      undefined,
      TickRank.Minor,
      undefined,
      undefined,
      undefined,
      TickRank.Major,
    ]);
    expect(ranks(5, 2)).toEqual([
      TickRank.Major,
      undefined,
      TickRank.Minor,
      undefined,
      TickRank.Major,
    ]);
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
