import { type KeyboardEvent, useRef, useState } from 'react';
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
  const draft = useRef(value);
  const submitted = useRef(value);

  const edit = (next: string) => {
    draft.current = next;
    setName(next);
  };

  const submit = () => {
    const trimmed = draft.current.trim();
    if (!trimmed || trimmed === submitted.current) return;
    submitted.current = trimmed;
    onSubmit(trimmed);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Escape') {
      edit(submitted.current);
      return;
    }
    if (event.key !== 'Enter') return;
    event.preventDefault();
    submit();
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
        onChange={(event) => edit(event.target.value)}
        onBlur={submit}
        onKeyDown={handleKeyDown}
      />
    </label>
  );
}
