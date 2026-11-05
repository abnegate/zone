import { describe, expect, it } from 'bun:test';
import type { Handover, ToolCallRecord } from '../types';
import { splitTurn } from './turn';

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

const call = (id: string, at?: number): ToolCallRecord => ({
  id,
  name: 'read_file',
  arguments: '{}',
  success: true,
  detail: 'ok',
  duration_ms: 1,
  at,
});

const describeParts = (parts: ReturnType<typeof splitTurn>): string[] =>
  parts.map((part) => {
    if (part.kind === 'text') return `text:${part.text}`;
    if (part.kind === 'calls') return `calls:${part.calls.map((item) => item.id).join(',')}`;
    return `handover:${part.handover.to}`;
  });

describe('splitTurn with tool calls', () => {
  it('draws each call after the text written before it, not above all the text', () => {
    const content = 'Let me look. Found it. Now the other one. Done.';

    const parts = splitTurn(content, [call('first', 13), call('second', 23)], []);

    expect(describeParts(parts)).toEqual([
      'text:Let me look. ',
      'calls:first',
      'text:Found it. ',
      'calls:second',
      'text:Now the other one. Done.',
    ]);
  });

  it('keeps calls made back to back in one group, in the order they ran', () => {
    const parts = splitTurn('Checking. Both read.', [call('a', 10), call('b', 10)], []);

    expect(describeParts(parts)).toEqual(['text:Checking. ', 'calls:a,b', 'text:Both read.']);
  });

  it('opens the turn with calls made before any text', () => {
    const parts = splitTurn('Answer', [call('a', 0)], []);

    expect(describeParts(parts)).toEqual(['calls:a', 'text:Answer']);
  });

  it('draws a call stored without an offset at the top, where it always was', () => {
    const parts = splitTurn('Answer', [call('old')], []);

    expect(describeParts(parts)).toEqual(['calls:old', 'text:Answer']);
  });

  it('shows a call still running at the end of what has streamed so far', () => {
    const parts = splitTurn('Let me look.', [call('running', 12)], []);

    expect(describeParts(parts)).toEqual(['text:Let me look.', 'calls:running']);
  });

  it('counts code points as the server does', () => {
    const parts = splitTurn('🙂🙂 done', [call('a', 2)], []);

    expect(describeParts(parts)).toEqual(['text:🙂🙂', 'calls:a', 'text: done']);
  });

  it('places calls and handovers among the text by where each happened', () => {
    const moved = handover({ at: 4, to: 'c@example.com' });

    const parts = splitTurn('abcdefgh', [call('later', 6), call('early', 2)], [moved]);

    expect(describeParts(parts)).toEqual([
      'text:ab',
      'calls:early',
      'text:cd',
      'handover:c@example.com',
      'text:ef',
      'calls:later',
      'text:gh',
    ]);
  });

  it('keeps calls either side of a handover at the same offset apart', () => {
    const moved = handover({ at: 3 });

    const parts = splitTurn('abc', [call('before', 3)], [moved]);

    expect(describeParts(parts)).toEqual(['text:abc', 'calls:before', 'handover:b@example.com']);
  });
});

describe('splitTurn with handovers', () => {
  it('leaves an answer that never moved whole', () => {
    expect(splitTurn('Hello', [], [])).toEqual([{ kind: 'text', text: 'Hello', offset: 0 }]);
  });

  it('puts the divider where the new account took over', () => {
    const switched = handover({ at: 6 });

    expect(splitTurn('First half. Second half.', [], [switched])).toEqual([
      { kind: 'text', text: 'First ', offset: 0 },
      { kind: 'handover', handover: switched, agentChanged: false },
      { kind: 'text', text: 'half. Second half.', offset: 6 },
    ]);
  });

  it('counts code points as the server does, so an emoji is never halved', () => {
    const parts = splitTurn('🙂🙂 done', [], [handover({ at: 2 })]);

    expect(parts[0]).toEqual({ kind: 'text', text: '🙂🙂', offset: 0 });
    expect(parts[2]).toEqual({ kind: 'text', text: ' done', offset: 2 });
  });

  it('shows a switch made before any text with nothing ahead of it', () => {
    const parts = splitTurn('Answer', [], [handover({ at: 0 })]);

    expect(parts.map((part) => part.kind)).toEqual(['handover', 'text']);
  });

  it('shows a switch that has not written anything yet', () => {
    expect(splitTurn('', [], [handover({ at: 0 })]).map((part) => part.kind)).toEqual(['handover']);
  });

  it('orders switches by where they happened and clamps one past the end', () => {
    const first = handover({ at: 2, to: 'b@example.com' });
    const second = handover({
      at: 99,
      from: 'b@example.com',
      to: 'c@example.com',
    });

    const parts = splitTurn('abcd', [], [second, first]);

    expect(parts).toEqual([
      { kind: 'text', text: 'ab', offset: 0 },
      { kind: 'handover', handover: first, agentChanged: false },
      { kind: 'text', text: 'cd', offset: 2 },
      { kind: 'handover', handover: second, agentChanged: false },
    ]);
  });

  it('names the agent only on the switch that changed it', () => {
    const replayed = handover({ at: 1, carried: false });
    const toCodex = handover({
      at: 2,
      agent: 'codex',
      carried: false,
      to: 'c@example.com',
    });
    const stayed = handover({
      at: 3,
      from_agent: 'codex',
      agent: 'codex',
      carried: false,
      from: 'c@example.com',
      to: 'd@example.com',
    });

    const notices = splitTurn('abcd', [], [replayed, toCodex, stayed]).filter(
      (part) => part.kind === 'handover'
    );

    expect(notices.map((part) => part.agentChanged)).toEqual([false, true, false]);
  });
});
