import { describe, expect, it, mock } from 'bun:test';
import type { ClientRect, UniqueIdentifier } from '@dnd-kit/core';
import { STAGE_IDS, type StageId } from '../../status';
import type { Task, TaskStatus } from '../../types';
import { destination, stageSteps, stageUnder } from './drop';

const COLUMN_WIDTH = 280;
const COLUMN_GAP = 20;

function rect(left: number, width: number, top = 0, height = 600): ClientRect {
  return { left, width, top, height, right: left + width, bottom: top + height };
}

function column(stage: StageId): ClientRect {
  return rect(STAGE_IDS.indexOf(stage) * (COLUMN_WIDTH + COLUMN_GAP), COLUMN_WIDTH);
}

const RECTS = new Map<UniqueIdentifier, ClientRect>(STAGE_IDS.map((id) => [id, column(id)]));

function task(status: TaskStatus): Task {
  return { id: 'task-1', title: 'Ship it', status } as Task;
}

function key(code: string) {
  return { code, preventDefault: mock() } as unknown as KeyboardEvent;
}

describe('destination', () => {
  it('keeps a card dropped on its own column where it is', () => {
    expect(destination(task('in_progress'), 'in_progress')).toBeNull();
    expect(destination(task('cancelled'), 'cancelled')).toBeNull();
  });

  it('moves a card to the status its new column stands for', () => {
    expect(destination(task('in_progress'), 'planned')).toBe('created');
    expect(destination(task('created'), 'in_progress')).toBe('in_progress');
    expect(destination(task('blocked'), 'paused')).toBe('paused');
    expect(destination(task('review'), 'complete')).toBe('complete');
    expect(destination(task('complete'), 'cancelled')).toBe('cancelled');
  });

  it('leaves a queued card queued when it is dropped back on Planned', () => {
    expect(destination(task('queued'), 'planned')).toBeNull();
    expect(destination(task('created'), 'planned')).toBeNull();
  });
});

describe('stageSteps', () => {
  const steps = stageSteps(() => STAGE_IDS);

  function step(
    code: string,
    { over = null, status = 'in_progress' }: { over?: StageId | null; status?: TaskStatus } = {}
  ) {
    const event = key(code);
    const start = over ?? (status === 'queued' || status === 'created' ? 'planned' : status);
    const card = rect(column(start as StageId).left + 10, 260, 40, 80);
    const moved = steps(event, {
      active: 'task-1',
      currentCoordinates: { x: card.left, y: card.top },
      context: {
        active: { id: 'task-1', data: { current: { task: task(status) } } },
        over: over === null ? null : { id: over },
        collisionRect: card,
        droppableRects: RECTS,
      } as never,
    });
    return { moved, event, card };
  }

  it('moves the middle of the card onto the middle of the next column right', () => {
    const { moved, event, card } = step('ArrowRight');
    const target = column('blocked');
    expect(moved).toEqual({ x: target.left + 10, y: card.top });
    expect(event.preventDefault).toHaveBeenCalled();
  });

  it('moves the card one column left', () => {
    const { moved, card } = step('ArrowLeft');
    expect(moved).toEqual({ x: column('planned').left + 10, y: card.top });
  });

  it('steps from the column the card is over rather than where it started', () => {
    const { moved, card } = step('ArrowRight', { over: 'review' });
    expect(moved).toEqual({ x: column('complete').left + 10, y: card.top });
  });

  it('starts a queued card from Planned', () => {
    const { moved, card } = step('ArrowRight', { status: 'queued' });
    expect(moved).toEqual({ x: column('in_progress').left + 10, y: card.top });
  });

  it('stops at the first and last column', () => {
    expect(step('ArrowLeft', { status: 'created' }).moved).toBeUndefined();
    expect(step('ArrowRight', { status: 'cancelled' }).moved).toBeUndefined();
  });

  it('ignores the vertical arrows and other keys without claiming them', () => {
    for (const code of ['ArrowUp', 'ArrowDown', 'KeyA']) {
      const { moved, event } = step(code);
      expect(moved).toBeUndefined();
      expect(event.preventDefault).not.toHaveBeenCalled();
    }
  });

  it('reads the column order each time it moves', () => {
    let order: readonly StageId[] = STAGE_IDS;
    const live = stageSteps(() => order);
    const card = rect(column('in_progress').left + 10, 260, 40, 80);
    const args = {
      active: 'task-1',
      currentCoordinates: { x: card.left, y: card.top },
      context: {
        active: { id: 'task-1', data: { current: { task: task('in_progress') } } },
        over: null,
        collisionRect: card,
        droppableRects: RECTS,
      } as never,
    };
    order = ['in_progress', 'complete'];
    expect(live(key('ArrowRight'), args)).toEqual({ x: column('complete').left + 10, y: card.top });
  });
});

describe('stageUnder', () => {
  const containers = STAGE_IDS.map((id) => ({ id, rect: { current: column(id) } }));

  function collide(pointer: { x: number; y: number } | null, card: ClientRect) {
    return stageUnder({
      active: { id: 'task-1' } as never,
      collisionRect: card,
      droppableRects: RECTS,
      droppableContainers: containers as never,
      pointerCoordinates: pointer,
    }).map((collision) => collision.id);
  }

  it('finds the column under the pointer', () => {
    const card = rect(10, 260, 40, 80);
    expect(collide({ x: column('review').left + 5, y: 100 }, card)[0]).toBe('review');
  });

  it('falls back to the column the card overlaps most without a pointer', () => {
    const card = rect(column('paused').left + 40, 260, 40, 80);
    expect(collide(null, card)[0]).toBe('paused');
  });

  it('falls back to overlap when the pointer is between columns', () => {
    const card = rect(column('blocked').left + 10, 260, 40, 80);
    expect(collide({ x: column('blocked').right + 5, y: 100 }, card)[0]).toBe('blocked');
  });
});
