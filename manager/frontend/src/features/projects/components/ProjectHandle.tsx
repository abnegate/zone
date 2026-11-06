import { useProjectDrag } from '../hooks/useProjectDrag';

export interface ProjectHandleProps {
  name: string;
}

/** The grip a project card is dragged by, kept apart from the card so a click still opens it. */
export function ProjectHandle({ name }: ProjectHandleProps) {
  const drag = useProjectDrag();
  if (!drag) return null;

  return (
    <button
      type="button"
      ref={drag.setHandle}
      className="project-card-handle"
      title={`Move ${name}`}
      {...drag.attributes}
      {...drag.listeners}
      aria-label={`Move ${name}`}
      aria-describedby={drag.instructions}
      onClick={(event) => event.stopPropagation()}
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
