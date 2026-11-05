import { describe, expect, it } from 'bun:test';
import type { ChatSubagent } from '../subagentTypes';
import {
  formatDuration,
  instant,
  mergeSubagent,
  mergeSubagents,
  startedIn,
  stepStatusLabel,
  subagentActive,
  subagentDuration,
  subagentName,
  subagentStatusLabel,
  subagentStatusTint,
} from './subagents';

const REVIEWER: ChatSubagent = {
  id: 'subagent-1',
  chat_id: 'chat-1',
  message_id: 'message-1',
  external_id: 'toolu_01Agent',
  description: 'Ask for a review',
  kind: 'general-purpose',
  model: 'sonnet',
  background: false,
  status: 'running',
  tool_uses: 3,
  duration_ms: 4_000,
  tokens: 1_200,
  last_tool: 'Bash',
  summary: null,
  error: null,
  steps: [],
  started_at: '2026-11-05T10:00:00Z',
  finished_at: null,
  updated_at: '2026-11-05T10:00:05Z',
};

const TESTER: ChatSubagent = {
  ...REVIEWER,
  id: 'subagent-2',
  external_id: 'toolu_02Agent',
  description: 'Run the tests',
  started_at: '2026-11-05T10:01:00Z',
  updated_at: '2026-11-05T10:01:00Z',
};

describe('instant', () => {
  it('reads the moment a timestamp names, whatever its offset', () => {
    expect(instant('2026-11-05T12:00:00+02:00')).toBe(instant('2026-11-05T10:00:00Z'));
  });

  it('keeps the microseconds Postgres stores beyond the millisecond', () => {
    expect(instant('2026-11-05T10:00:00.123457Z')).toBeGreaterThan(
      instant('2026-11-05T10:00:00.123456Z')
    );
    expect(instant('2026-11-05T10:00:00.5Z')).toBe(instant('2026-11-05T10:00:00.500Z'));
  });

  it('has no moment for a missing or unreadable timestamp', () => {
    expect(instant(null)).toBeNaN();
    expect(instant('yesterday-ish')).toBeNaN();
  });
});

describe('mergeSubagent', () => {
  it('adds an agent it has not seen, in start order', () => {
    expect(mergeSubagent([TESTER], REVIEWER)).toEqual([REVIEWER, TESTER]);
  });

  it('replaces an agent with a copy written later', () => {
    const finished: ChatSubagent = {
      ...REVIEWER,
      status: 'completed',
      updated_at: '2026-11-05T10:00:09Z',
    };
    expect(mergeSubagent([REVIEWER, TESTER], finished)).toEqual([finished, TESTER]);
  });

  it('keeps the copy it has when the incoming one was written earlier', () => {
    const list = [REVIEWER, TESTER];
    const stale: ChatSubagent = { ...REVIEWER, tool_uses: 1, updated_at: '2026-11-05T10:00:01Z' };
    expect(mergeSubagent(list, stale)).toBe(list);
  });

  it('compares written times, not strings, across offsets', () => {
    const held: ChatSubagent = { ...REVIEWER, updated_at: '2026-11-05T10:30:00Z' };
    const stringLater: ChatSubagent = {
      ...REVIEWER,
      status: 'failed',
      updated_at: '2026-11-05T12:15:00+02:00',
    };
    expect(stringLater.updated_at > held.updated_at).toBe(true);
    expect(mergeSubagent([held], stringLater)).toEqual([held]);

    const stringEarlier: ChatSubagent = { ...REVIEWER, updated_at: '2026-11-05T11:00:00+02:00' };
    expect(held.updated_at < stringEarlier.updated_at).toBe(true);
    expect(mergeSubagent([stringEarlier], held)).toEqual([held]);
  });

  it('compares written times, not strings, across fractional digits', () => {
    const whole: ChatSubagent = { ...REVIEWER, updated_at: '2026-11-05T10:00:05Z' };
    const fraction: ChatSubagent = {
      ...REVIEWER,
      status: 'completed',
      updated_at: '2026-11-05T10:00:05.000001Z',
    };
    expect(fraction.updated_at < whole.updated_at).toBe(true);
    expect(mergeSubagent([whole], fraction)).toEqual([fraction]);
    expect(mergeSubagent([fraction], whole)).toEqual([fraction]);

    const short: ChatSubagent = { ...REVIEWER, updated_at: '2026-11-05T10:00:05.5Z' };
    const long: ChatSubagent = {
      ...REVIEWER,
      status: 'completed',
      updated_at: '2026-11-05T10:00:05.500001Z',
    };
    expect(long.updated_at < short.updated_at).toBe(true);
    expect(mergeSubagent([short], long)).toEqual([long]);
    expect(mergeSubagent([long], short)).toEqual([long]);
  });

  it('orders by start time, not by string, and then by id', () => {
    const offset: ChatSubagent = {
      ...TESTER,
      id: 'subagent-3',
      started_at: '2026-11-05T11:30:00+02:00',
    };
    const twin: ChatSubagent = { ...REVIEWER, id: 'subagent-0' };
    expect(mergeSubagents([], [TESTER, REVIEWER, offset, twin]).map(({ id }) => id)).toEqual([
      'subagent-3',
      'subagent-0',
      'subagent-1',
      'subagent-2',
    ]);
  });
});

describe('mergeSubagents', () => {
  it('folds a loaded list in by the same keep-newer rule', () => {
    const live: ChatSubagent = {
      ...REVIEWER,
      status: 'completed',
      updated_at: '2026-11-05T10:00:09Z',
    };
    const merged = mergeSubagents([live], [REVIEWER, TESTER]);
    expect(merged).toEqual([live, TESTER]);
  });

  it('keeps an agent the loaded list does not mention', () => {
    expect(mergeSubagents([REVIEWER], [TESTER])).toEqual([REVIEWER, TESTER]);
  });

  it('hands back the same list when nothing changed', () => {
    const list = [REVIEWER, TESTER];
    expect(mergeSubagents(list, [REVIEWER, { ...TESTER }])).not.toBe(list);
    expect(mergeSubagents(list, list)).toBe(list);
    expect(mergeSubagents(list, [])).toBe(list);
  });
});

describe('subagent readings', () => {
  it('finds the agents a message started', () => {
    const other: ChatSubagent = { ...TESTER, message_id: 'message-2' };
    const orphan: ChatSubagent = { ...TESTER, id: 'subagent-4', message_id: null };
    expect(startedIn([REVIEWER, other, orphan], 'message-1')).toEqual([REVIEWER]);
    expect(startedIn([REVIEWER, other, orphan], 'message-3')).toEqual([]);
  });

  it('names an agent by its description, or plainly when it has none', () => {
    expect(subagentName(REVIEWER)).toBe('Ask for a review');
    expect(subagentName({ ...REVIEWER, description: '  ' })).toBe('Agent');
  });

  it('counts only a running agent as active', () => {
    expect(subagentActive(REVIEWER)).toBe(true);
    for (const status of ['completed', 'failed', 'stopped'] as const)
      expect(subagentActive({ ...REVIEWER, status })).toBe(false);
  });

  it('names and tints every status', () => {
    expect(subagentStatusLabel('running')).toBe('Running');
    expect(subagentStatusLabel('completed')).toBe('Completed');
    expect(subagentStatusLabel('failed')).toBe('Failed');
    expect(subagentStatusLabel('stopped')).toBe('Stopped');
    expect(subagentStatusTint('running')).toBe('info');
    expect(subagentStatusTint('completed')).toBe('success');
    expect(subagentStatusTint('failed')).toBe('destructive');
    expect(subagentStatusTint('stopped')).toBe('neutral');
    expect(stepStatusLabel('running')).toBe('Running');
    expect(stepStatusLabel('succeeded')).toBe('Succeeded');
    expect(stepStatusLabel('failed')).toBe('Failed');
  });
});

describe('subagentDuration', () => {
  const now = Date.parse('2026-11-05T10:00:42Z');

  it('counts a running agent live from its start', () => {
    expect(subagentDuration(REVIEWER, now)).toBe(42_000);
  });

  it('never shows a running agent shorter than the time it reported', () => {
    expect(subagentDuration({ ...REVIEWER, duration_ms: 60_000 }, now)).toBe(60_000);
  });

  it('never counts a running agent below zero when the clocks disagree', () => {
    expect(
      subagentDuration({ ...REVIEWER, duration_ms: null }, Date.parse('2026-11-05T09:59:00Z'))
    ).toBe(0);
  });

  it('reads a finished agent as it reported, else from its stored times', () => {
    const finished: ChatSubagent = {
      ...REVIEWER,
      status: 'completed',
      duration_ms: 53_000,
      finished_at: '2026-11-05T10:01:00Z',
    };
    expect(subagentDuration(finished, now)).toBe(53_000);
    expect(subagentDuration({ ...finished, duration_ms: null }, now)).toBe(60_000);
    expect(subagentDuration({ ...finished, duration_ms: null, finished_at: null }, now)).toBeNull();
  });
});

describe('formatDuration', () => {
  it('reads seconds, minutes and hours', () => {
    expect(formatDuration(0)).toBe('0s');
    expect(formatDuration(999)).toBe('0s');
    expect(formatDuration(42_000)).toBe('42s');
    expect(formatDuration(185_000)).toBe('3m 05s');
    expect(formatDuration(3_720_000)).toBe('1h 02m');
  });

  it('reads nonsense as no time at all', () => {
    expect(formatDuration(-5_000)).toBe('0s');
    expect(formatDuration(Number.NaN)).toBe('0s');
  });
});
