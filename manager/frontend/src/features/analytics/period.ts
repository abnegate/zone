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

export const PERIODS: readonly PeriodOption[] = [
  TimePeriodSchema.enum.week,
  TimePeriodSchema.enum.month,
  TimePeriodSchema.enum.quarter,
].map((value) => ({ value, label: LABELS[value] }));

export function isPeriod(value: string): value is TimePeriod {
  return TimePeriodSchema.safeParse(value).success;
}

export function periodLabel(period: TimePeriod): string {
  return LABELS[period];
}
