import { useBoxDrag } from '../hooks/useBoxDrag';

export interface BoxHandleProps {
  title: string;
}

/** The grip a panel box is dragged by, kept apart from its header so a click still collapses it. */
export function BoxHandle({ title }: BoxHandleProps) {
  const drag = useBoxDrag();
  if (!drag) return null;

  return (
    <button
      type="button"
      ref={drag.setHandle}
      className="chat-panel-box-handle"
      title={`Move ${title}`}
      {...drag.attributes}
      {...drag.listeners}
      aria-label={`Move ${title}`}
      aria-describedby={drag.instructions}
    >
      <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
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
