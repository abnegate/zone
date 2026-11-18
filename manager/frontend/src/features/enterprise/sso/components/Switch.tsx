import { type KeyboardEvent, useId } from 'react';

const ACTIVATION_KEYS: ReadonlySet<string> = new Set([' ', 'Enter']);

interface SwitchProps {
  label: string;
  description?: string;
  hint?: string;
  checked: boolean;
  disabled?: boolean;
  onChange(checked: boolean): void;
}

export function Switch({
  label,
  description,
  hint,
  checked,
  disabled = false,
  onChange,
}: SwitchProps) {
  const controlId = useId();
  const labelId = useId();
  const descriptionId = useId();
  const hintId = useId();
  const describedBy = [description && descriptionId, hint && hintId].filter(Boolean).join(' ');

  const toggle = () => {
    if (!disabled) {
      onChange(!checked);
    }
  };

  const press = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!ACTIVATION_KEYS.has(event.key)) {
      return;
    }
    event.preventDefault();
    if (!event.repeat) {
      toggle();
    }
  };

  return (
    <div className="sso-switch">
      <button
        id={controlId}
        type="button"
        role="switch"
        className="sso-switch-control"
        aria-checked={checked}
        aria-labelledby={labelId}
        aria-describedby={describedBy || undefined}
        disabled={disabled}
        onClick={toggle}
        onKeyDown={press}
      >
        <span className="sso-switch-thumb" aria-hidden="true" />
      </button>
      <div className="sso-switch-text">
        <label id={labelId} htmlFor={controlId} className="sso-switch-label">
          {label}
        </label>
        {description && (
          <p id={descriptionId} className="sso-switch-description">
            {description}
          </p>
        )}
        {hint && (
          <p id={hintId} className="sso-switch-description">
            {hint}
          </p>
        )}
      </div>
    </div>
  );
}
