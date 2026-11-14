import { describe, expect, it } from 'bun:test';
import type { Handover } from '../types';
import { agentChanged, appendHandover, handoverCause, handoverTarget, resetsIn } from './handover';

const NOW = Date.parse('2026-09-23T04:00:00Z');
const MINUTE = 60_000;

const handover = (overrides: Partial<Handover> = {}): Handover => ({
  kind: 'handover',
  from: 'a@example.com',
  to: 'b@example.com',
  from_agent: 'claude',
  agent: 'claude',
  reason: 'limit',
  carried: true,
  at: 0,
  ...overrides,
});

describe('agentChanged', () => {
  it('a carried session never changes agent', () => {
    expect(agentChanged(handover({ carried: true }))).toBe(false);
  });

  it('a first switch that replayed on the same agent did not change it', () => {
    expect(agentChanged(handover({ carried: false }))).toBe(false);
  });

  it('a switch onto another agent changed it, whichever way it went', () => {
    expect(agentChanged(handover({ agent: 'codex', carried: false }))).toBe(true);
    expect(agentChanged(handover({ from_agent: 'codex', carried: false }))).toBe(true);
  });
});

describe('appendHandover', () => {
  it('appends a new switch', () => {
    const first = handover({ at: 1 });
    const second = handover({ at: 5, to: 'c@example.com' });

    expect(appendHandover([first], second)).toEqual([first, second]);
  });

  it('does not repeat a switch a replayed turn sends again', () => {
    const first = handover({ at: 1 });

    expect(appendHandover([first], { ...first })).toEqual([first]);
  });
});

describe('resetsIn', () => {
  it('reads hours and minutes', () => {
    expect(resetsIn(new Date(NOW + 130 * MINUTE).toISOString(), NOW)).toBe('2h 10m');
  });

  it('rounds a part minute up rather than claiming it has already reset', () => {
    expect(resetsIn(new Date(NOW + 30_000).toISOString(), NOW)).toBe('1m');
  });

  it('reads whole hours and days without empty units', () => {
    expect(resetsIn(new Date(NOW + 120 * MINUTE).toISOString(), NOW)).toBe('2h');
    expect(resetsIn(new Date(NOW + (26 * 60 + 5) * MINUTE).toISOString(), NOW)).toBe('1d 2h');
    expect(resetsIn(new Date(NOW + 48 * 60 * MINUTE).toISOString(), NOW)).toBe('2d');
  });

  it('says nothing about a reset that has passed, is missing or is unreadable', () => {
    expect(resetsIn(new Date(NOW - MINUTE).toISOString(), NOW)).toBeUndefined();
    expect(resetsIn(undefined, NOW)).toBeUndefined();
    expect(resetsIn('soon', NOW)).toBeUndefined();
  });
});

describe('handover wording', () => {
  const resets_at = new Date(NOW + 130 * MINUTE).toISOString();

  it('names the agent only when it changed', () => {
    expect(handoverTarget(handover({ agent: 'codex' }), true)).toBe('Codex · b@example.com');
    expect(handoverTarget(handover({ agent: 'codex' }), false)).toBe('b@example.com');
  });

  it('says the account reached its limit and when it resets', () => {
    expect(handoverCause(handover({ resets_at }), NOW)).toBe(
      'a@example.com reached its usage limit; resets in 2h 10m'
    );
  });

  it('says the account ran out of credits', () => {
    expect(handoverCause(handover({ reason: 'credits' }), NOW)).toBe(
      'a@example.com ran out of credits'
    );
  });

  it('says a signed-out account was signed out, with no reset to wait for', () => {
    expect(handoverCause(handover({ reason: 'signed_out', resets_at }), NOW)).toBe(
      'a@example.com was signed out'
    );
  });

  it('says a chat left another agent for the configured one, with no reset to wait for', () => {
    expect(handoverCause(handover({ reason: 'configured', resets_at }), NOW)).toBe(
      'a@example.com gave way to the configured agent'
    );
  });

  it('speaks of the previous account when the switch names none', () => {
    expect(handoverCause(handover({ from: undefined }), NOW)).toBe(
      'the previous account reached its usage limit'
    );
  });
});
