import { useDraggable, useDroppable } from '@dnd-kit/core';
import { type ReactNode, useCallback } from 'react';
import { ProjectHandle } from './ProjectHandle';

export type DropSide = 'before' | 'after';

export interface ProjectSlotProps {
  id: string;
  name: string;
  movable: boolean;
  instructions: string;
  drop: DropSide | null;
  children: ReactNode;
}

/** The card's grip sits beside the card, not in it, so the card stays one control. */
export function ProjectSlot({ id, name, movable, instructions, drop, children }: ProjectSlotProps) {
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

  return (
    <div
      ref={setSlot}
      className="project-slot"
      data-project={id}
      data-drop={drop ?? undefined}
      data-dragging={isDragging || undefined}
    >
      {children}
      {movable && (
        <ProjectHandle
          name={name}
          setHandle={setActivatorNodeRef}
          attributes={attributes}
          listeners={listeners}
          instructions={instructions}
        />
      )}
    </div>
  );
}
