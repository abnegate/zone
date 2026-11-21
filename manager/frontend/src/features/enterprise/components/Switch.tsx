import { type KeyboardEvent, useId } from 'react';
import './Switch.css';

const ACTIVATION_KEYS: ReadonlySet<string> = new Set([' ', 'Enter']);

interface SwitchProps {
  label: string;
  ariaLabel?: string;
  description?: string;
  hint?: string;
  checked: boolean;
  disabled?: boolean;
  onChange(checked: boolean): void;
}

export function Switch({
  label,
  ariaLabel,
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
    <div className="enterprise-switch">
      <button
        id={controlId}
        type="button"
        role="switch"
        className="enterprise-switch-control"
        aria-checked={checked}
        aria-label={ariaLabel}
        aria-labelledby={ariaLabel ? undefined : labelId}
        aria-describedby={describedBy || undefined}
        disabled={disabled}
        onClick={toggle}
        onKeyDown={press}
      >
        <span className="enterprise-switch-thumb" aria-hidden="true" />
      </button>
      <div className="enterprise-switch-text">
        <label id={labelId} htmlFor={controlId} className="enterprise-switch-label">
          {label}
        </label>
        {description && (
          <p id={descriptionId} className="enterprise-switch-description">
            {description}
          </p>
        )}
        {hint && (
          <p id={hintId} className="enterprise-switch-description">
            {hint}
          </p>
        )}
      </div>
    </div>
  );
}
