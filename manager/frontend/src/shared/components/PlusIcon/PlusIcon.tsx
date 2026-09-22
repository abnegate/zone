import type { ReactElement } from 'react';

export default function PlusIcon(): ReactElement {
  return (
    <svg
      className="plus-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      width="16"
      height="16"
      aria-hidden="true"
    >
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}
