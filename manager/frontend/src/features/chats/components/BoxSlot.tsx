import { useDraggable, useDroppable } from '@dnd-kit/core';
import { type ReactNode, useCallback, useMemo } from 'react';
import { BoxDragContext } from '../hooks/useBoxDrag';
import type { DropSide, PanelBoxId } from '../utils/boxes';

export interface BoxSlotProps {
  id: PanelBoxId;
  movable: boolean;
  drop: DropSide | null;
  children: ReactNode;
}

/** Where one box sits in the panel: what it is dragged by, and where another can be dropped beside it. */
export function BoxSlot({ id, movable, drop, children }: BoxSlotProps) {
  const { attributes, listeners, isDragging, setNodeRef, setActivatorNodeRef } = useDraggable({
    id,
    disabled: !movable,
    attributes: { roleDescription: 'drag handle' },
  });
  const droppable = useDroppable({ id, disabled: !movable });
  const setDroppable = droppable.setNodeRef;

  const setSlot = useCallback(
    (element: HTMLElement | null) => {
      setNodeRef(element);
      setDroppable(element);
    },
    [setNodeRef, setDroppable]
  );

  const drag = useMemo(
    () =>
      movable
        ? { setHandle: setActivatorNodeRef, attributes, listeners, dragging: isDragging }
        : null,
    [movable, setActivatorNodeRef, attributes, listeners, isDragging]
  );

  return (
    <div
      ref={setSlot}
      className="chat-panel-slot"
      data-box={id}
      data-drop={drop ?? undefined}
      data-dragging={isDragging || undefined}
    >
      <BoxDragContext value={drag}>{children}</BoxDragContext>
    </div>
  );
}
