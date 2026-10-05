import { describe, expect, it } from 'bun:test';
import type { Handover } from '../types';
import {
  agentChanged,
  appendHandover,
  handoverCause,
  handoverTarget,
  resetsIn,
  splitAtHandovers,
} from './handover';

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

describe('splitAtHandovers', () => {
  it('leaves an answer that never moved whole', () => {
    expect(splitAtHandovers('Hello', [])).toEqual([{ kind: 'text', text: 'Hello', offset: 0 }]);
  });

  it('puts the divider where the new account took over', () => {
    const switched = handover({ at: 6 });

    expect(splitAtHandovers('First half. Second half.', [switched])).toEqual([
      { kind: 'text', text: 'First ', offset: 0 },
      { kind: 'handover', handover: switched, agentChanged: false },
      { kind: 'text', text: 'half. Second half.', offset: 6 },
    ]);
  });

  it('counts code points as the server does, so an emoji is never halved', () => {
    const parts = splitAtHandovers('🙂🙂 done', [handover({ at: 2 })]);

    expect(parts[0]).toEqual({ kind: 'text', text: '🙂🙂', offset: 0 });
    expect(parts[2]).toEqual({ kind: 'text', text: ' done', offset: 2 });
  });

  it('shows a switch made before any text with nothing ahead of it', () => {
    const parts = splitAtHandovers('Answer', [handover({ at: 0 })]);

    expect(parts.map((part) => part.kind)).toEqual(['handover', 'text']);
  });

  it('shows a switch that has not written anything yet', () => {
    expect(splitAtHandovers('', [handover({ at: 0 })]).map((part) => part.kind)).toEqual([
      'handover',
    ]);
  });

  it('orders switches by where they happened and clamps one past the end', () => {
    const first = handover({ at: 2, to: 'b@example.com' });
    const second = handover({ at: 99, from: 'b@example.com', to: 'c@example.com' });

    const parts = splitAtHandovers('abcd', [second, first]);

    expect(parts).toEqual([
      { kind: 'text', text: 'ab', offset: 0 },
      { kind: 'handover', handover: first, agentChanged: false },
      { kind: 'text', text: 'cd', offset: 2 },
      { kind: 'handover', handover: second, agentChanged: false },
    ]);
  });

  it('names the agent only on the switch that changed it', () => {
    const replayed = handover({ at: 1, carried: false });
    const toCodex = handover({ at: 2, agent: 'codex', carried: false, to: 'c@example.com' });
    const stayed = handover({
      at: 3,
      from_agent: 'codex',
      agent: 'codex',
      carried: false,
      from: 'c@example.com',
      to: 'd@example.com',
    });

    const notices = splitAtHandovers('abcd', [replayed, toCodex, stayed]).filter(
      (part) => part.kind === 'handover'
    );

    expect(notices.map((part) => part.agentChanged)).toEqual([false, true, false]);
  });
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
