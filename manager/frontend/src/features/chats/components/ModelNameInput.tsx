import { type KeyboardEvent, useEffect, useState } from 'react';
import { MODEL_NAME_PLACEHOLDER } from '../utils';

const IME_KEY_CODE = 229;

export interface ModelNameInputProps {
  provider: string;
  value: string;
  busy?: boolean;
  onSubmit: (name: string) => void;
  onCancel?: () => void;
}

export function ModelNameInput({
  provider,
  value,
  busy = false,
  onSubmit,
  onCancel,
}: ModelNameInputProps) {
  const [name, setName] = useState(value);
  const trimmed = name.trim();

  useEffect(() => {
    setName(value);
  }, [value]);

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing || event.keyCode === IME_KEY_CODE) return;
    if (event.key === 'Escape') {
      if (busy) return;
      setName(value);
      onCancel?.();
      return;
    }
    if (event.key !== 'Enter') return;
    event.preventDefault();
    if (!busy && trimmed && trimmed !== value) onSubmit(trimmed);
  };

  return (
    <label className="chat-provider-model-name" data-testid="chat-provider-model-name">
      <span className="chat-header-select-label">Model</span>
      <input
        type="text"
        aria-label="Provider model name"
        title={`Which model ${provider} runs. AI Settings save none for it, so name one it serves and press Enter.`}
        placeholder={MODEL_NAME_PLACEHOLDER}
        value={name}
        readOnly={busy}
        aria-busy={busy}
        spellCheck={false}
        autoComplete="off"
        onChange={(event) => setName(event.target.value)}
        onKeyDown={handleKeyDown}
      />
      {!busy && trimmed && trimmed !== value && (
        <span className="chat-provider-model-hint">Enter to save</span>
      )}
    </label>
  );
}
