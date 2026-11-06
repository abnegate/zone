import { useDraggable, useDroppable } from '@dnd-kit/core';
import { type ReactNode, useCallback, useMemo } from 'react';
import { ProjectDragContext } from '../hooks/useProjectDrag';

export type DropSide = 'before' | 'after';

export interface ProjectSlotProps {
  id: string;
  movable: boolean;
  instructions: string;
  drop: DropSide | null;
  children: ReactNode;
}

/** Where one project sits in the list: what it is dragged by, and where another can be dropped beside it. */
export function ProjectSlot({ id, movable, instructions, drop, children }: ProjectSlotProps) {
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
      movable ? { setHandle: setActivatorNodeRef, attributes, listeners, instructions } : null,
    [movable, setActivatorNodeRef, attributes, listeners, instructions]
  );

  return (
    <div
      ref={setSlot}
      className="project-slot"
      data-project={id}
      data-drop={drop ?? undefined}
      data-dragging={isDragging || undefined}
    >
      <ProjectDragContext value={drag}>{children}</ProjectDragContext>
    </div>
  );
}
