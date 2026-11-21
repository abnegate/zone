import { DateSchema } from './schemas';

export interface SpendRange {
  from: string;
  to: string;
}

export const MAXIMUM_DAYS = 366;

export const RangeProblem = {
  Invalid: 'invalid',
  TooLong: 'too_long',
} as const;

export type RangeProblem = (typeof RangeProblem)[keyof typeof RangeProblem];

const DATE_LENGTH = 10;

const FIRST_DAY = 1;

const DAY_START = 'T00:00:00Z';

const MILLISECONDS_PER_DAY = 86_400_000;

function isoDate(date: Date): string {
  return date.toISOString().slice(0, DATE_LENGTH);
}

export function monthToDate(now: Date = new Date()): SpendRange {
  const first = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), FIRST_DAY));
  return { from: isoDate(first), to: isoDate(now) };
}

function startOf(value: string): Date {
  return new Date(`${value}${DAY_START}`);
}

export function validDate(value: string): boolean {
  if (!DateSchema.safeParse(value).success) return false;
  const date = startOf(value);
  return !Number.isNaN(date.getTime()) && isoDate(date) === value;
}

function days({ from, to }: SpendRange): number {
  return (startOf(to).getTime() - startOf(from).getTime()) / MILLISECONDS_PER_DAY + 1;
}

export function rangeProblem(range: SpendRange): RangeProblem | null {
  if (!validDate(range.from) || !validDate(range.to) || range.from > range.to) {
    return RangeProblem.Invalid;
  }
  return days(range) > MAXIMUM_DAYS ? RangeProblem.TooLong : null;
}

export function validRange(range: SpendRange): boolean {
  return rangeProblem(range) === null;
}
