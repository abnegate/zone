import { DateSchema } from './schemas';

export interface SpendRange {
  from: string;
  to: string;
}

const DATE_LENGTH = 10;

const FIRST_DAY = 1;

const DAY_START = 'T00:00:00Z';

function isoDate(date: Date): string {
  return date.toISOString().slice(0, DATE_LENGTH);
}

export function monthToDate(now: Date = new Date()): SpendRange {
  const first = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), FIRST_DAY));
  return { from: isoDate(first), to: isoDate(now) };
}

export function validDate(value: string): boolean {
  if (!DateSchema.safeParse(value).success) return false;
  const date = new Date(`${value}${DAY_START}`);
  return !Number.isNaN(date.getTime()) && isoDate(date) === value;
}

export function validRange({ from, to }: SpendRange): boolean {
  return validDate(from) && validDate(to) && from <= to;
}
