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

const call = (
  id: string,
  at?: number,
  overrides: Partial<ToolCallRecord> = {}
): ToolCallRecord => ({
  id,
  name: 'read_file',
  arguments: '{}',
  success: true,
  detail: 'ok',
  duration_ms: 1,
  at,
  ...overrides,
});

const describeParts = (parts: ReturnType<typeof splitTurn>): string[] =>
  parts.map((part) => {
    if (part.kind === 'text') return `text:${part.text}`;
    if (part.kind === 'calls') {
      const ids = part.calls.map((item) => `${item.id}${item.reasoning ? '+thought' : ''}`);
      return `calls:${ids.join(',')}${part.reasoning ? `|${part.reasoning}` : ''}`;
    }
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

  it('keeps a call made before a handover at the same offset above the divider', () => {
    const moved = handover({ at: 3, calls: 1 });

    const parts = splitTurn('abc', [call('before', 3)], [moved]);

    expect(describeParts(parts)).toEqual(['text:abc', 'calls:before', 'handover:b@example.com']);
  });

  it('draws a call the new login made at the same offset below the divider', () => {
    const moved = handover({ at: 3, calls: 1 });

    const parts = splitTurn('abc', [call('old', 3), call('new', 3)], [moved]);

    expect(describeParts(parts)).toEqual([
      'text:abc',
      'calls:old',
      'handover:b@example.com',
      'calls:new',
    ]);
  });

  it('keeps what the login being left thought above the divider', () => {
    const moved = handover({ at: 5, calls: 0, reasoning: 'Out of room.' });

    const parts = splitTurn('Hello there', [], [moved], { text: 'Fresh start.', at: 5 });

    expect(describeParts(parts)).toEqual([
      'text:Hello',
      'calls:|Out of room.',
      'handover:b@example.com',
      'calls:|Fresh start.',
      'text: there',
    ]);
  });

  it('pairs what the login being left thought with the call it made last', () => {
    const moved = handover({ at: 5, calls: 1, reasoning: 'Out of room.' });

    const parts = splitTurn('Hello', [call('old', 5)], [moved]);

    expect(describeParts(parts)).toEqual([
      'text:Hello',
      'calls:old|Out of room.',
      'handover:b@example.com',
    ]);
  });
});

describe('splitTurn with thinking', () => {
  it('draws thinking where it began when text came between it and its call', () => {
    const content = 'Opened. Then wrote this. ';
    const later = call('later', 25, { reasoning: 'Look next.', reasoning_at: 8 });

    const parts = splitTurn(content, [call('first', 0), later], []);

    expect(describeParts(parts)).toEqual([
      'calls:first',
      'text:Opened. ',
      'calls:|Look next.',
      'text:Then wrote this. ',
      'calls:later',
    ]);
  });

  it('keeps thinking beside its call when nothing was written between them', () => {
    const parts = splitTurn(
      'Done.',
      [call('a', 0, { reasoning: 'Read it.', reasoning_at: 0 })],
      []
    );

    expect(describeParts(parts)).toEqual(['calls:a+thought', 'text:Done.']);
  });

  it('keeps thinking beside its call when it was stored without an offset', () => {
    const parts = splitTurn('Hi. Done.', [call('a', 4, { reasoning: 'Read it.' })], []);

    expect(describeParts(parts)).toEqual(['text:Hi. ', 'calls:a+thought', 'text:Done.']);
  });

  it('draws thinking after the last call below that call, even when the call had none', () => {
    const calls = [call('a', 0, { reasoning: 'First.', reasoning_at: 0 }), call('b', 5)];

    const parts = splitTurn('Mid. Final.', calls, [], { text: 'Now answer.' });

    expect(describeParts(parts)).toEqual([
      'calls:a+thought',
      'text:Mid. ',
      'calls:b|Now answer.',
      'text:Final.',
    ]);
  });

  it('draws thinking still streaming after the text it followed', () => {
    const parts = splitTurn('Looked. Thinking more', [call('a', 0)], [], {
      text: 'Hmm.',
      at: 8,
    });

    expect(describeParts(parts)).toEqual([
      'calls:a',
      'text:Looked. ',
      'calls:|Hmm.',
      'text:Thinking more',
    ]);
  });

  it('opens the turn with thinking when there were no calls and no offset', () => {
    const parts = splitTurn('Answer', [], [], { text: 'Think.' });

    expect(describeParts(parts)).toEqual(['calls:|Think.', 'text:Answer']);
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
