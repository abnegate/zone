import { utcInstant } from '../../../shared/utils/time';
import { TIME_ZONE } from '../AnalyticsFormat';
import { TimePeriodSchema } from '../schemas';
import type { TimePeriod, TimeWindow } from '../types';

const STEPS = [1, 2, 5, 10] as const;
const BASE = 10;
const HALF = 2;

export const TickRank = {
  Major: 'major',
  Minor: 'minor',
} as const;

export type TickRank = (typeof TickRank)[keyof typeof TickRank];

const days = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  timeZone: TIME_ZONE,
});
const hours = new Intl.DateTimeFormat(undefined, { hour: 'numeric', timeZone: TIME_ZONE });
const moments = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  hour: 'numeric',
  timeZone: TIME_ZONE,
  timeZoneName: 'short',
});

const NOTES: Record<TimePeriod, string> = {
  day: 'Hours in UTC',
  week: 'Days in UTC',
  month: 'Days in UTC',
  quarter: 'Weeks in UTC',
};

function hourly(period: TimePeriod): boolean {
  return period === TimePeriodSchema.enum.day;
}

function formatted(window: TimeWindow, format: Intl.DateTimeFormat): string {
  const start = utcInstant(window.start);
  return start ? format.format(start) : window.start;
}

export const RunSeriesAxis = {
  ceiling(value: number): number {
    if (value <= 1) return 1;
    const magnitude = BASE ** Math.floor(Math.log10(value));
    const step = STEPS.find((candidate) => candidate * magnitude >= value) ?? BASE;
    return step * magnitude;
  },

  gridlines(ceiling: number): number[] {
    const middle = ceiling / HALF;
    return Number.isInteger(middle) ? [ceiling, middle, 0] : [ceiling, 0];
  },

  labelEvery(points: number, labels: number): number {
    return Math.max(1, Math.ceil(points / labels));
  },

  rank(index: number, every: number): TickRank | undefined {
    if (index % every !== 0) return undefined;
    return (index / every) % HALF === 0 ? TickRank.Major : TickRank.Minor;
  },

  tick(window: TimeWindow, period: TimePeriod): string {
    return formatted(window, hourly(period) ? hours : days);
  },

  bucket(window: TimeWindow, period: TimePeriod): string {
    return formatted(window, hourly(period) ? moments : days);
  },

  note(period: TimePeriod): string {
    return NOTES[period];
  },
} as const;
