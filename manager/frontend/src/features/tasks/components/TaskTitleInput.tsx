import { type KeyboardEvent, type SyntheticEvent, useEffect, useRef, useState } from 'react';

interface TaskTitleInputProps {
  title: string;
  className?: string;
  onSave: (title: string) => Promise<void>;
  onDone: () => void;
}

function stopPropagation(event: SyntheticEvent) {
  event.stopPropagation();
}

export function TaskTitleInput({ title, className, onSave, onDone }: TaskTitleInputProps) {
  const [value, setValue] = useState(title);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const settled = useRef(false);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  const finish = () => {
    settled.current = true;
    onDone();
  };

  const commit = async () => {
    if (settled.current || saving) return;
    const trimmed = value.trim();
    if (!trimmed || trimmed === title) {
      finish();
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await onSave(trimmed);
      finish();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to rename task');
      setSaving(false);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    event.stopPropagation();
    if (event.key === 'Enter') {
      event.preventDefault();
      void commit();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      finish();
    }
  };

  return (
    <span className={`task-title-edit ${className ?? ''}`}>
      <input
        ref={input}
        type="text"
        className="task-title-input"
        aria-label="Task title"
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? 'task-title-error' : undefined}
        value={value}
        readOnly={saving}
        aria-busy={saving || undefined}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={handleKeyDown}
        onClick={stopPropagation}
        onBlur={() => void commit()}
      />
      {error && (
        <span id="task-title-error" className="field-error" role="alert">
          {error}
        </span>
      )}
    </span>
  );
}
