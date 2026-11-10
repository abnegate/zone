import {
  type ClientRect,
  type CollisionDetection,
  type KeyboardCoordinateGetter,
  pointerWithin,
  rectIntersection,
} from '@dnd-kit/core';
import { STAGES, type StageId, stageOf } from '../../status';
import type { Task, TaskStatus } from '../../types';

const KEY_STEPS: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1 };

function centre(rect: ClientRect): number {
  return rect.left + rect.width / 2;
}

export function stageSteps(order: () => readonly StageId[]): KeyboardCoordinateGetter {
  return (event, { currentCoordinates, context }) => {
    const step = KEY_STEPS[event.code];
    if (!step) return undefined;
    event.preventDefault();
    const task: Task | undefined = context.active?.data.current?.task;
    const start = context.over?.id ?? (task && stageOf(task.status).id);
    const stages = order();
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

export const stageUnder: CollisionDetection = (args) => {
  const within = pointerWithin(args);
  return within.length > 0 ? within : rectIntersection(args);
};

export function destination(task: Task, stageId: StageId): TaskStatus | null {
  if (stageOf(task.status).id === stageId) return null;
  return STAGES.find((stage) => stage.id === stageId)?.target ?? null;
}
