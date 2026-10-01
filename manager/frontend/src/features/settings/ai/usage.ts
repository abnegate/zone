import { format, isSameDay } from 'date-fns';
import type { AgentAccount, UsageWindow } from './schemas';

export const Severity = {
  Fine: 'fine',
  Warning: 'warning',
  Critical: 'critical',
  Exhausted: 'exhausted',
  Unknown: 'unknown',
} as const;

export type Severity = (typeof Severity)[keyof typeof Severity];

const WARNING_PERCENT = 75;
const CRITICAL_PERCENT = 90;
const EXHAUSTED_PERCENT = 100;
const UNNAMED = 'Unnamed account';

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export function severityOf(usedPercent: number | null): Severity {
  if (usedPercent === null) return Severity.Unknown;
  if (usedPercent >= EXHAUSTED_PERCENT) return Severity.Exhausted;
  if (usedPercent >= CRITICAL_PERCENT) return Severity.Critical;
  if (usedPercent >= WARNING_PERCENT) return Severity.Warning;
  return Severity.Fine;
}

export function percentOf(window: UsageWindow): string {
  return window.used_percent === null ? '?%' : `${Math.round(window.used_percent)}%`;
}

export function filledOf(window: UsageWindow): number {
  return window.used_percent === null ? 0 : Math.min(Math.max(window.used_percent, 0), 100);
}

export function countsOf(window: UsageWindow): string {
  return window.used !== null && window.limit !== null ? `${window.used}/${window.limit}` : '';
}

export function resetsIn(at: string | null, now: number): string {
  if (at === null) return '';
  const remaining = Date.parse(at) - now;
  if (Number.isNaN(remaining)) return '';
  if (remaining < 1000) return 'resets now';

  const days = Math.floor(remaining / DAY);
  const hours = Math.floor(remaining / HOUR) % 24;
  const minutes = Math.floor(remaining / MINUTE) % 60;

  if (days > 0) return `resets in ${days}d ${hours}h`;
  if (hours > 0) return `resets in ${hours}h ${minutes}m`;
  return `resets in ${minutes}m`;
}

export function accountLabel(account: Pick<AgentAccount, 'label' | 'plan'>): string {
  return account.label ?? account.plan ?? UNNAMED;
}

export function exhaustedUntil(account: AgentAccount, now: number): string | null {
  const until = account.usage?.exhausted_until ?? null;
  return until !== null && Date.parse(until) > now ? until : null;
}

export function moment(at: string, now: number): string {
  const date = new Date(at);
  return isSameDay(date, now) ? format(date, 'p') : format(date, 'MMM d, p');
}
