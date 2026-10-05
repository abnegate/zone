import { describe, expect, it } from 'bun:test';
import fixture from '../../../../../../runner/zone_server/tests/fixtures/agents.json';
import { AgentStatusesSchema, type UsageWindow } from './schemas';
import {
  accountLabel,
  countsOf,
  exhaustedUntil,
  filledOf,
  moment,
  percentOf,
  resetsIn,
  Severity,
  severityOf,
} from './usage';

const now = Date.parse('2026-09-23T04:00:00Z');
const [account] = AgentStatusesSchema.parse(fixture).agents[0].logins;
const window = (changes: Partial<UsageWindow> = {}): UsageWindow => ({
  name: '5h',
  used_percent: 62,
  used: null,
  limit: null,
  resets_at: null,
  ...changes,
});
const at = (milliseconds: number) => new Date(now + milliseconds).toISOString();

describe('usage', () => {
  it('escalates the severity with the share used, as aiusg paints it', () => {
    expect(severityOf(10)).toBe(Severity.Fine);
    expect(severityOf(74.9)).toBe(Severity.Fine);
    expect(severityOf(75)).toBe(Severity.Warning);
    expect(severityOf(80)).toBe(Severity.Warning);
    expect(severityOf(90)).toBe(Severity.Critical);
    expect(severityOf(95)).toBe(Severity.Critical);
    expect(severityOf(100)).toBe(Severity.Exhausted);
    expect(severityOf(150)).toBe(Severity.Exhausted);
    expect(severityOf(null)).toBe(Severity.Unknown);
  });

  it('reads the share used as a whole percentage, and an unknown one as a question', () => {
    expect(percentOf(window())).toBe('62%');
    expect(percentOf(window({ used_percent: 41.6 }))).toBe('42%');
    expect(percentOf(window({ used_percent: null }))).toBe('?%');
  });

  it('fills a bar no further than full and no less than empty', () => {
    expect(filledOf(window())).toBe(62);
    expect(filledOf(window({ used_percent: 150 }))).toBe(100);
    expect(filledOf(window({ used_percent: -5 }))).toBe(0);
    expect(filledOf(window({ used_percent: null }))).toBe(0);
  });

  it('counts requests only when both the used and the limit are known', () => {
    expect(countsOf(window({ used: 12, limit: 50 }))).toBe('12/50');
    expect(countsOf(window({ used: 12 }))).toBe('');
    expect(countsOf(window({ limit: 50 }))).toBe('');
    expect(countsOf(window())).toBe('');
  });

  it('reads reset times as durations', () => {
    expect(resetsIn(at(42 * 60_000), now)).toBe('resets in 42m');
    expect(resetsIn(at(3 * 3_600_000), now)).toBe('resets in 3h 0m');
    expect(resetsIn(at(50 * 3_600_000), now)).toBe('resets in 2d 2h');
    expect(resetsIn('2026-09-23T06:10:00Z', now)).toBe('resets in 2h 10m');
    expect(resetsIn('2026-09-28T04:00:00Z', now)).toBe('resets in 5d 0h');
  });

  it('reads a reset that has passed as now, and an unknown one as nothing', () => {
    expect(resetsIn(at(-3_600_000), now)).toBe('resets now');
    expect(resetsIn(at(500), now)).toBe('resets now');
    expect(resetsIn(null, now)).toBe('');
    expect(resetsIn('not a time', now)).toBe('');
  });

  it('names an account by its label, then its plan', () => {
    expect(accountLabel(account)).toBe('jake@example.com');
    expect(accountLabel({ label: null, plan: 'Claude Max' })).toBe('Claude Max');
    expect(accountLabel({ label: null, plan: null })).toBe('Unnamed account');
  });

  it('holds an account exhausted only until its reset', () => {
    expect(exhaustedUntil(account, now)).toBeNull();
    const spent = { ...account, exhausted_until: '2026-09-23T06:10:00Z' };
    expect(exhaustedUntil(spent, now)).toBe('2026-09-23T06:10:00Z');
    expect(exhaustedUntil(spent, Date.parse('2026-09-23T06:10:00Z'))).toBeNull();
  });

  it('holds an account exhausted though its usage was never read', () => {
    const unread = { ...account, usage: null, exhausted_until: '2026-09-23T06:10:00Z' };
    expect(exhaustedUntil(unread, now)).toBe('2026-09-23T06:10:00Z');
  });

  it('names a time today by the clock, and another day by its date too', () => {
    expect(moment('2026-09-23T03:50:00Z', now)).toBe('3:50 AM');
    expect(moment('2026-09-28T04:00:00Z', now)).toBe('Sep 28, 4:00 AM');
  });
});
