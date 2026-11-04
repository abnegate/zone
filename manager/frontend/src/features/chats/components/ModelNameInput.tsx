import { type KeyboardEvent, useState } from 'react';
import { MODEL_NAME_PLACEHOLDER } from '../utils';

export interface ModelNameInputProps {
  provider: string;
  value: string;
  disabled?: boolean;
  onSubmit: (name: string) => void;
}

export function ModelNameInput({
  provider,
  value,
  disabled = false,
  onSubmit,
}: ModelNameInputProps) {
  const [name, setName] = useState(value);
  const trimmed = name.trim();

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Escape') {
      setName(value);
      return;
    }
    if (event.key !== 'Enter') return;
    event.preventDefault();
    if (trimmed && trimmed !== value) onSubmit(trimmed);
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
        disabled={disabled}
        spellCheck={false}
        autoComplete="off"
        onChange={(event) => setName(event.target.value)}
        onKeyDown={handleKeyDown}
      />
      {trimmed && trimmed !== value && (
        <span className="chat-provider-model-hint">Enter to save</span>
      )}
    </label>
  );
}
