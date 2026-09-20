import { Label } from '@zone/ui';
import { type DragEvent, type ReactElement, useState } from 'react';
import './DropZone.css';

type DropZoneProps = {
  id: string;
  label: string;
  prompt: string;
  hint: string;
  accept: string;
  disabled?: boolean;
  onFiles: (files: File[]) => void;
};

function accepts(accept: string, file: File): boolean {
  return accept.split(',').some((pattern) => {
    const wanted = pattern.trim();
    return wanted.endsWith('/*') ? file.type.startsWith(wanted.slice(0, -1)) : file.type === wanted;
  });
}

export default function DropZone({
  id,
  label,
  prompt,
  hint,
  accept,
  disabled = false,
  onFiles,
}: DropZoneProps): ReactElement {
  const [over, setOver] = useState(false);
  const labelId = `${id}-label`;
  const hintId = `${id}-hint`;

  const handleDragOver = (event: DragEvent<HTMLLabelElement>) => {
    if (disabled) return;
    event.preventDefault();
    setOver(true);
  };

  const handleDrop = (event: DragEvent<HTMLLabelElement>) => {
    event.preventDefault();
    setOver(false);
    if (disabled) return;
    const dropped = Array.from(event.dataTransfer.files).filter((file) => accepts(accept, file));
    if (dropped.length > 0) onFiles(dropped);
  };

  return (
    <div className="drop-zone-field">
      <Label id={labelId} htmlFor={id}>
        {label}
      </Label>
      <label
        className={`drop-zone${over ? ' drop-zone--over' : ''}`}
        htmlFor={id}
        data-disabled={disabled || undefined}
        onDragOver={handleDragOver}
        onDragLeave={() => setOver(false)}
        onDrop={handleDrop}
      >
        <input
          id={id}
          className="drop-zone-input"
          type="file"
          accept={accept}
          multiple
          disabled={disabled}
          aria-labelledby={labelId}
          aria-describedby={hintId}
          onChange={(event) => {
            const chosen = Array.from(event.target.files ?? []);
            event.target.value = '';
            if (chosen.length > 0) onFiles(chosen);
          }}
        />
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M12 16V4M6 10l6-6 6 6M4 20h16" />
        </svg>
        <span className="drop-zone-prompt">{prompt}</span>
      </label>
      <p id={hintId} className="drop-zone-hint">
        {hint}
      </p>
    </div>
  );
}
