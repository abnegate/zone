import { TimePeriodSchema } from './schemas';
import type { TimePeriod } from './types';

export interface PeriodOption {
  value: TimePeriod;
  label: string;
}

const LABELS: Record<TimePeriod, string> = {
  day: '24 hours',
  week: '7 days',
  month: '30 days',
  quarter: '90 days',
};

export const DEFAULT_PERIOD: TimePeriod = TimePeriodSchema.enum.week;

export const PERIOD_PARAM = 'period';

export const PERIODS: readonly PeriodOption[] = TimePeriodSchema.options.map((value) => ({
  value,
  label: LABELS[value],
}));

export function isPeriod(value: string | null): value is TimePeriod {
  return TimePeriodSchema.safeParse(value).success;
}

export function periodOf(value: string | null): TimePeriod {
  return isPeriod(value) ? value : DEFAULT_PERIOD;
}

export function periodLabel(period: TimePeriod): string {
  return LABELS[period];
}
