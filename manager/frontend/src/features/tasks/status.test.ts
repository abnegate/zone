import { describe, expect, it } from 'bun:test';
import type { StageId } from './status';
import {
  groupByStage,
  STAGE_IDS,
  STAGES,
  STATUS_OPTIONS,
  stageOf,
  statusLabel,
  statusTint,
  TASK_STATUSES,
} from './status';

describe('stages', () => {
  it('lists the seven board columns in order with their labels', () => {
    expect(STAGES.map((stage) => [stage.id, stage.label])).toEqual([
      ['planned', 'Planned'],
      ['in_progress', 'In progress'],
      ['blocked', 'Blocked'],
      ['paused', 'Paused'],
      ['review', 'In review'],
      ['complete', 'Completed'],
      ['cancelled', 'Cancelled'],
    ]);
    expect(STAGES.map((stage) => stage.id)).toEqual([...STAGE_IDS]);
  });

  it('groups created and queued into planned, which moves cards to created', () => {
    expect(stageOf('created').id).toBe('planned');
    expect(stageOf('queued').id).toBe('planned');
    expect(stageOf('created').statuses).toEqual(['created', 'queued']);
    expect(stageOf('created').target).toBe('created');
  });

  it('puts every status in exactly one stage', () => {
    for (const status of TASK_STATUSES) {
      const holders = STAGES.filter((stage) => stage.statuses.includes(status));
      expect(holders).toHaveLength(1);
      expect(stageOf(status)).toBe(holders[0]);
    }
  });

  it('targets a status the stage holds', () => {
    for (const stage of STAGES) {
      expect(stage.statuses).toContain(stage.target);
      expect(stage.tint).toBe(statusTint(stage.target));
    }
  });
});

describe('statuses', () => {
  it('labels and tints each status', () => {
    expect(
      TASK_STATUSES.map((status) => [status, statusLabel(status), statusTint(status)])
    ).toEqual([
      ['created', 'Created', 'neutral'],
      ['queued', 'Queued', 'info'],
      ['in_progress', 'In progress', 'info'],
      ['blocked', 'Blocked', 'destructive'],
      ['paused', 'Paused', 'neutral'],
      ['review', 'In review', 'warning'],
      ['complete', 'Completed', 'success'],
      ['cancelled', 'Cancelled', 'neutral'],
    ]);
  });

  it('offers every status as an option in order', () => {
    expect(STATUS_OPTIONS).toEqual(
      TASK_STATUSES.map((value) => ({ value, label: statusLabel(value) }))
    );
    expect(STATUS_OPTIONS.map((option) => option.value)).toEqual([
      'created',
      'queued',
      'in_progress',
      'blocked',
      'paused',
      'review',
      'complete',
      'cancelled',
    ]);
  });
});

describe('groupByStage', () => {
  it('returns every stage, including empty ones, in order', () => {
    const groups = groupByStage([]);
    expect([...groups.keys()]).toEqual([...STAGE_IDS]);
    for (const tasks of groups.values()) {
      expect(tasks).toEqual([]);
    }
  });

  it('keeps the order the tasks arrived in', () => {
    const tasks = [
      { id: 'a', status: 'queued' },
      { id: 'b', status: 'complete' },
      { id: 'c', status: 'created' },
      { id: 'd', status: 'cancelled' },
      { id: 'e', status: 'queued' },
      { id: 'f', status: 'paused' },
    ] as const;
    const groups = groupByStage(tasks);
    const ids = (id: StageId) => groups.get(id)?.map((task) => task.id);
    expect(ids('planned')).toEqual(['a', 'c', 'e']);
    expect(ids('complete')).toEqual(['b']);
    expect(ids('cancelled')).toEqual(['d']);
    expect(ids('paused')).toEqual(['f']);
    expect(ids('in_progress')).toEqual([]);
  });
});
