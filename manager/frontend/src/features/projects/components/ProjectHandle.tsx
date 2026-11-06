import type { DraggableAttributes, DraggableSyntheticListeners } from '@dnd-kit/core';

export interface ProjectHandleProps {
  name: string;
  setHandle: (element: HTMLElement | null) => void;
  attributes: DraggableAttributes;
  listeners: DraggableSyntheticListeners;
  instructions: string;
}

export function ProjectHandle({
  name,
  setHandle,
  attributes,
  listeners,
  instructions,
}: ProjectHandleProps) {
  return (
    <button
      type="button"
      ref={setHandle}
      className="project-card-handle"
      title={`Move ${name}`}
      {...attributes}
      {...listeners}
      aria-label={`Move ${name}`}
      aria-describedby={instructions}
    >
      <svg viewBox="6 3 12 18" fill="currentColor" aria-hidden="true">
        <circle cx="9" cy="6" r="1.5" />
        <circle cx="15" cy="6" r="1.5" />
        <circle cx="9" cy="12" r="1.5" />
        <circle cx="15" cy="12" r="1.5" />
        <circle cx="9" cy="18" r="1.5" />
        <circle cx="15" cy="18" r="1.5" />
      </svg>
    </button>
  );
}
