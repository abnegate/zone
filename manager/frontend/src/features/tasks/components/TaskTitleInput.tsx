import { type KeyboardEvent, type SyntheticEvent, useEffect, useId, useRef, useState } from 'react';

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
  const original = useRef(title);
  const [value, setValue] = useState(title);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const settled = useRef(false);
  const input = useRef<HTMLInputElement>(null);
  const errorId = useId();

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
    if (!trimmed || trimmed === original.current) {
      finish();
      return;
    }
    setSaving(true);
    setMessage(null);
    try {
      await onSave(trimmed);
      finish();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Failed to rename task');
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
    <span className={['task-title-edit', className].filter(Boolean).join(' ')}>
      <input
        ref={input}
        type="text"
        className="task-title-input"
        aria-label="Task title"
        aria-invalid={message ? true : undefined}
        aria-describedby={message ? errorId : undefined}
        value={value}
        readOnly={saving}
        aria-busy={saving || undefined}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={handleKeyDown}
        onClick={stopPropagation}
        onBlur={() => void commit()}
      />
      {message && (
        <span id={errorId} className="field-error" role="alert">
          {message}
        </span>
      )}
    </span>
  );
}
