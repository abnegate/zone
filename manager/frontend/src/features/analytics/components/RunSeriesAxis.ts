import { utcInstant } from '../../../shared/utils/time';
import { TimePeriodSchema } from '../schemas';
import type { TimePeriod, TimeWindow } from '../types';

const STEPS = [1, 2, 5, 10] as const;
const BASE = 10;
const UTC = 'UTC';

const days = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', timeZone: UTC });
const hours = new Intl.DateTimeFormat(undefined, { hour: 'numeric', timeZone: UTC });
const moments = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  hour: 'numeric',
  timeZone: UTC,
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

  labelEvery(points: number, labels: number): number {
    return Math.max(1, Math.ceil(points / labels));
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
