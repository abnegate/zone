import { describe, expect, it } from 'bun:test';
import type { ChatSubagent, SubagentStep } from '../subagentTypes';
import {
  absentSubagents,
  formatDuration,
  groupByMessage,
  instant,
  mergeSteps,
  mergeSubagent,
  mergeSubagents,
  stepStatusLabel,
  subagentActive,
  subagentDuration,
  subagentLabel,
  subagentName,
  subagentStatusLabel,
  subagentStatusTint,
  withoutSubagents,
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
  step_count: 0,
  revision: 2,
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

  it('replaces an agent with a copy of a higher revision', () => {
    const finished: ChatSubagent = { ...REVIEWER, status: 'completed', revision: 3 };
    expect(mergeSubagent([REVIEWER, TESTER], finished)).toEqual([finished, TESTER]);
  });

  it('keeps the copy it has when the incoming one has a lower revision', () => {
    const list = [REVIEWER, TESTER];
    const stale: ChatSubagent = { ...REVIEWER, tool_uses: 1, revision: 1 };
    expect(mergeSubagent(list, stale)).toBe(list);
  });

  it('takes the incoming copy when the revisions tie', () => {
    const twin: ChatSubagent = { ...REVIEWER, last_tool: 'Read' };
    expect(mergeSubagent([REVIEWER], twin)).toEqual([twin]);
  });

  it('orders copies by revision alone, whatever their written times say', () => {
    const later: ChatSubagent = {
      ...REVIEWER,
      status: 'completed',
      revision: 3,
      updated_at: '2026-11-05T09:00:00Z',
    };
    const earlier: ChatSubagent = { ...REVIEWER, updated_at: '2026-11-05T11:00:00Z' };
    expect(mergeSubagent([later], earlier)).toEqual([later]);
    expect(mergeSubagent([earlier], later)).toEqual([later]);
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
  it('folds a loaded list in by the same higher-revision rule', () => {
    const live: ChatSubagent = { ...REVIEWER, status: 'completed', revision: 5 };
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

describe('withoutSubagents', () => {
  it('drops the agents named, and hands back the same list when none were held', () => {
    const list = [REVIEWER, TESTER];
    expect(withoutSubagents(list, new Set([REVIEWER.id, 'unknown']))).toEqual([TESTER]);
    expect(withoutSubagents(list, new Set(['unknown']))).toBe(list);
  });
});

describe('absentSubagents', () => {
  it('names the agents that are not kept', () => {
    const list = [REVIEWER, TESTER];
    expect(absentSubagents(list, new Set([TESTER.id, 'unknown']))).toEqual(new Set([REVIEWER.id]));
    expect(absentSubagents(list, new Set([REVIEWER.id, TESTER.id]))).toEqual(new Set());
    expect(absentSubagents([], new Set())).toEqual(new Set());
  });
});

describe('mergeSteps', () => {
  const READ: SubagentStep = { id: 'toolu_1', name: 'Read', subject: 'a.rs', status: 'succeeded' };
  const BASH: SubagentStep = {
    id: 'toolu_2',
    name: 'Bash',
    subject: 'cargo test',
    status: 'running',
  };
  const GREP: SubagentStep = { id: 'toolu_3', name: 'Grep', subject: null, status: 'running' };

  it('updates a step in place and appends new ones in arrival order', () => {
    const passed: SubagentStep = { ...BASH, status: 'succeeded' };
    const listed: SubagentStep = { ...GREP, id: 'toolu_4', name: 'Glob' };
    expect(mergeSteps([READ, BASH], [GREP, passed, listed])).toEqual([READ, passed, GREP, listed]);
  });

  it('never moves a settled step back to running', () => {
    const failed: SubagentStep = { ...BASH, status: 'failed' };
    const list = [READ, failed];
    expect(mergeSteps(list, [BASH, { ...READ, status: 'running' }])).toBe(list);
  });

  it('hands back the same list when nothing changed', () => {
    const list = [READ, BASH];
    expect(mergeSteps(list, [])).toBe(list);
    expect(mergeSteps(list, list)).toBe(list);
  });
});

describe('subagent readings', () => {
  it('groups the agents by the message that started them, in list order', () => {
    const other: ChatSubagent = { ...TESTER, message_id: 'message-2' };
    const orphan: ChatSubagent = { ...TESTER, id: 'subagent-4', message_id: null };
    const sibling: ChatSubagent = { ...TESTER, id: 'subagent-5' };
    const grouped = groupByMessage([REVIEWER, other, orphan, sibling]);
    expect([...grouped.keys()]).toEqual(['message-1', 'message-2']);
    expect(grouped.get('message-1')).toEqual([REVIEWER, sibling]);
    expect(grouped.get('message-2')).toEqual([other]);
    expect(grouped.get('message-3')).toBeUndefined();
  });

  it('names an agent by its description, or plainly when it has none', () => {
    expect(subagentName(REVIEWER)).toBe('Ask for a review');
    expect(subagentName({ ...REVIEWER, description: '  ' })).toBe('Agent');
  });

  it('labels an agent by its description, or plainly when it has none', () => {
    expect(subagentLabel(REVIEWER)).toBe('Agent: Ask for a review');
    expect(subagentLabel({ ...REVIEWER, description: '' })).toBe('Agent');
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
