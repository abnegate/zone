import {
  type ClientRect,
  type CollisionDetection,
  type KeyboardCoordinateGetter,
  type KeyboardSensorOptions,
  pointerWithin,
  rectIntersection,
} from '@dnd-kit/core';
import { STAGE_IDS, STAGES, type StageId, stageOf } from '../../status';
import type { Task, TaskStatus } from '../../types';

const KEY_STEPS: Partial<Record<string, number>> = { ArrowLeft: -1, ArrowRight: 1 };

const HELD_KEYS: ReadonlySet<string> = new Set(['ArrowUp', 'ArrowDown']);

const KEYBOARD_CODES = { start: ['Space'], cancel: ['Escape'], end: ['Space', 'Enter'] };

function centre(rect: ClientRect): number {
  return rect.left + rect.width / 2;
}

export function stageSteps(stages: readonly StageId[]): KeyboardCoordinateGetter {
  return (event, { currentCoordinates, context }) => {
    if (HELD_KEYS.has(event.code)) {
      event.preventDefault();
      return undefined;
    }
    const step = KEY_STEPS[event.code];
    if (!step) return undefined;
    event.preventDefault();
    const task: Task | undefined = context.active?.data.current?.task;
    const start = context.over?.id ?? (task && stageOf(task.status).id);
    const index = stages.indexOf(start as StageId);
    if (index === -1) return undefined;
    const target = stages[index + step];
    const rect = target === undefined ? undefined : context.droppableRects.get(target);
    const { collisionRect } = context;
    if (!rect || !collisionRect) return undefined;
    return {
      x: currentCoordinates.x + centre(rect) - centre(collisionRect),
      y: currentCoordinates.y,
    };
  };
}

export function keyboardOptions(reducedMotion: boolean): KeyboardSensorOptions {
  return {
    coordinateGetter: stageSteps(STAGE_IDS),
    keyboardCodes: KEYBOARD_CODES,
    scrollBehavior: reducedMotion ? 'auto' : 'smooth',
  };
}

export const stageUnder: CollisionDetection = (args) => {
  const within = pointerWithin(args);
  return within.length > 0 ? within : rectIntersection(args);
};

export function destination(task: Task, stageId: StageId): TaskStatus | null {
  if (stageOf(task.status).id === stageId) return null;
  return STAGES.find((stage) => stage.id === stageId)?.target ?? null;
}
