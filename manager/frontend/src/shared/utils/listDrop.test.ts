import { describe, expect, it, mock } from 'bun:test';
import type { ClientRect, DroppableContainer, UniqueIdentifier } from '@dnd-kit/core';
import { slotSteps, nearestSlot } from './listDrop';

function rect(top: number, height: number): ClientRect {
  return { top, height, bottom: top + height, left: 0, right: 300, width: 300 };
}

const RECTS = new Map<UniqueIdentifier, ClientRect>([
  ['changes', rect(0, 40)],
  ['sources', rect(52, 200)],
  ['gallery', rect(264, 120)],
]);

const CONTAINERS = [...RECTS.keys()].map((id) => ({ id }) as DroppableContainer);

function collide(y: number | null, collisionRect: ClientRect = rect(0, 40)) {
  return nearestSlot({
    active: { id: 'changes' } as never,
    collisionRect,
    droppableRects: RECTS,
    droppableContainers: CONTAINERS,
    pointerCoordinates: y === null ? null : { x: 10, y },
  }).map((collision) => collision.id);
}

function key(code: string) {
  return { code, preventDefault: mock() } as unknown as KeyboardEvent;
}

describe('nearestSlot', () => {
  it('finds the box under the pointer', () => {
    expect(collide(20)).toEqual(['changes']);
    expect(collide(200)).toEqual(['sources']);
    expect(collide(300)).toEqual(['gallery']);
  });

  it('finds the nearest box when the pointer is between boxes or past the list', () => {
    expect(collide(44)).toEqual(['changes']);
    expect(collide(260)).toEqual(['gallery']);
    expect(collide(900)).toEqual(['gallery']);
    expect(collide(-50)).toEqual(['changes']);
  });

  it('follows the middle of the moved box when there is no pointer', () => {
    expect(collide(null, rect(250, 40))).toEqual(['gallery']);
  });

  it('finds nothing in an empty list', () => {
    expect(
      nearestSlot({
        active: { id: 'changes' } as never,
        collisionRect: rect(0, 40),
        droppableRects: new Map(),
        droppableContainers: [],
        pointerCoordinates: { x: 0, y: 0 },
      })
    ).toEqual([]);
  });
});

describe('slotSteps', () => {
  const steps = slotSteps(() => ['changes', 'sources', 'gallery']);

  function step(code: string, over: UniqueIdentifier | null, collisionRect = rect(0, 40)) {
    const event = key(code);
    const moved = steps(event, {
      active: 'changes',
      currentCoordinates: { x: 0, y: collisionRect.top },
      context: {
        over: over === null ? null : { id: over },
        collisionRect,
        droppableRects: RECTS,
      } as never,
    });
    return { moved, event };
  }

  it('moves the middle of the box onto the middle of the next box down', () => {
    const { moved, event } = step('ArrowDown', null);

    expect(moved).toEqual({ x: 0, y: 52 + 100 - 20 });
    expect(event.preventDefault).toHaveBeenCalled();
  });

  it('steps on from the box it is already over', () => {
    expect(step('ArrowDown', 'sources', rect(132, 40)).moved).toEqual({ x: 0, y: 264 + 60 - 20 });
    expect(step('ArrowUp', 'sources', rect(132, 40)).moved).toEqual({ x: 0, y: 0 });
  });

  it('stays put past either end of the list', () => {
    expect(step('ArrowUp', null).moved).toBeUndefined();
    expect(step('ArrowDown', 'gallery', rect(304, 40)).moved).toBeUndefined();
  });

  it('leaves other keys alone', () => {
    const { moved, event } = step('ArrowLeft', null);

    expect(moved).toBeUndefined();
    expect(event.preventDefault).not.toHaveBeenCalled();
  });
});
