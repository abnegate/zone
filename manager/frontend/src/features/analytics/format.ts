import { TimePeriodSchema } from './schemas';
import type { TimePeriod, TimeWindow } from './types';

export const NOT_MEASURED = '—';

const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3_600;
const SECONDS_PER_DAY = 86_400;
const PERCENT = 100;
const MILLISECOND_DIGITS = /(\.\d{3})\d+$/;

const counts = new Intl.NumberFormat();
const decimals = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });
const shares = new Intl.NumberFormat(undefined, { style: 'percent', maximumFractionDigits: 0 });
const days = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' });
const hours = new Intl.DateTimeFormat(undefined, { hour: 'numeric' });

export function utc(value: string): Date {
  return new Date(`${value.replace(MILLISECOND_DIGITS, '$1')}Z`);
}

export function count(value: number): string {
  return counts.format(value);
}

export function decimal(value: number | null): string {
  return value === null ? NOT_MEASURED : decimals.format(value);
}

export function share(value: number | null): string {
  return value === null ? NOT_MEASURED : shares.format(value);
}

export function points(change: number): string {
  const rounded = Math.round(Math.abs(change) * PERCENT);
  return `${rounded} ${rounded === 1 ? 'point' : 'points'}`;
}

export function duration(seconds: number): string {
  if (seconds < SECONDS_PER_MINUTE) return `${Math.round(seconds)}s`;
  if (seconds < SECONDS_PER_HOUR) return `${Math.round(seconds / SECONDS_PER_MINUTE)}m`;
  if (seconds < SECONDS_PER_DAY) {
    const whole = Math.floor(seconds / SECONDS_PER_HOUR);
    const minutes = Math.round((seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE);
    return minutes === 0 ? `${whole}h` : `${whole}h ${minutes}m`;
  }
  const whole = Math.floor(seconds / SECONDS_PER_DAY);
  const remainder = Math.round((seconds % SECONDS_PER_DAY) / SECONDS_PER_HOUR);
  return remainder === 0 ? `${whole}d` : `${whole}d ${remainder}h`;
}

export function measured(seconds: number, samples: number): string {
  return samples === 0 ? NOT_MEASURED : duration(seconds);
}

export function minutes(value: number | null): string {
  return value === null ? NOT_MEASURED : duration(value * SECONDS_PER_MINUTE);
}

export function bucketLabel(window: TimeWindow, period: TimePeriod): string {
  const start = utc(window.start);
  return period === TimePeriodSchema.enum.day ? hours.format(start) : days.format(start);
}

export function moment(value: string): string {
  return days.format(utc(value));
}
