import type { CollisionDetection, KeyboardCoordinateGetter, UniqueIdentifier } from '@dnd-kit/core';

const KEY_STEPS: Record<string, number> = { ArrowUp: -1, ArrowDown: 1 };

/** The slot under the pointer, or under the middle of the item being moved by keyboard, else the nearest. */
export const nearestSlot: CollisionDetection = ({
  collisionRect,
  droppableRects,
  droppableContainers,
  pointerCoordinates,
}) => {
  const y = pointerCoordinates?.y ?? collisionRect.top + collisionRect.height / 2;
  let nearest: { container: (typeof droppableContainers)[number]; distance: number } | null = null;
  for (const container of droppableContainers) {
    const rect = droppableRects.get(container.id);
    if (!rect) continue;
    const distance = Math.max(rect.top - y, y - rect.bottom, 0);
    if (!nearest || distance < nearest.distance) nearest = { container, distance };
  }
  return nearest
    ? [
        {
          id: nearest.container.id,
          data: { droppableContainer: nearest.container, value: nearest.distance },
        },
      ]
    : [];
};

/** Moves an item picked up by keyboard one place up or down a vertical list with each arrow key. */
export function slotSteps(ids: () => readonly UniqueIdentifier[]): KeyboardCoordinateGetter {
  return (event, { active, currentCoordinates, context }) => {
    const step = KEY_STEPS[event.code];
    if (!step) return undefined;
    event.preventDefault();
    const order = ids();
    const target = order[order.indexOf(context.over?.id ?? active) + step];
    const rect = target === undefined ? undefined : context.droppableRects.get(target);
    const { collisionRect } = context;
    if (!rect || !collisionRect) return undefined;
    const middle = (box: { top: number; height: number }) => box.top + box.height / 2;
    return {
      x: currentCoordinates.x,
      y: currentCoordinates.y + middle(rect) - middle(collisionRect),
    };
  };
}
