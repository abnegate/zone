import { utcInstant } from '../../shared/utils/time';

export const NOT_MEASURED = '—';

export const TIME_ZONE = 'UTC';

const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3_600;
const SECONDS_PER_DAY = 86_400;
const PERCENT = 100;

const counts = new Intl.NumberFormat();
const decimals = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });
const shares = new Intl.NumberFormat(undefined, { style: 'percent', maximumFractionDigits: 0 });
const dates = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  timeZone: TIME_ZONE,
});

function duration(seconds: number): string {
  if (seconds < SECONDS_PER_MINUTE) return `${Math.round(seconds)}s`;
  if (seconds < SECONDS_PER_HOUR) return `${Math.round(seconds / SECONDS_PER_MINUTE)}m`;
  if (seconds < SECONDS_PER_DAY) {
    const hours = Math.floor(seconds / SECONDS_PER_HOUR);
    const minutes = Math.round((seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE);
    return minutes === 0 ? `${hours}h` : `${hours}h ${minutes}m`;
  }
  const days = Math.floor(seconds / SECONDS_PER_DAY);
  const hours = Math.round((seconds % SECONDS_PER_DAY) / SECONDS_PER_HOUR);
  return hours === 0 ? `${days}d` : `${days}d ${hours}h`;
}

export const AnalyticsFormat = {
  count(value: number): string {
    return counts.format(value);
  },

  decimal(value: number | null): string {
    return value === null ? NOT_MEASURED : decimals.format(value);
  },

  share(value: number | null): string {
    return value === null ? NOT_MEASURED : shares.format(value);
  },

  points(change: number): string {
    const rounded = Math.round(Math.abs(change) * PERCENT);
    return `${rounded} ${rounded === 1 ? 'point' : 'points'}`;
  },

  duration,

  measured(seconds: number, samples: number): string {
    return samples === 0 ? NOT_MEASURED : duration(seconds);
  },

  minutes(value: number | null): string {
    return value === null ? NOT_MEASURED : duration(value * SECONDS_PER_MINUTE);
  },

  date(value: string): string {
    const date = utcInstant(value);
    return date ? dates.format(date) : value;
  },
} as const;
