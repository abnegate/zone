import { type KeyboardEvent, useRef } from 'react';

export const READINESS_FILTER_LABEL = 'Filter by readiness';

const NEXT_KEYS: readonly string[] = ['ArrowRight', 'ArrowDown'];

const PREVIOUS_KEYS: readonly string[] = ['ArrowLeft', 'ArrowUp'];

const ACTIVE = 'active';

const INACTIVE = 'inactive';

export interface ReadinessOption<Value extends string> {
  value: Value;
  label: string;
}

export interface ReadinessFilterProps<Value extends string> {
  options: readonly ReadinessOption<Value>[];
  value: Value;
  onChange: (value: Value) => void;
}

export function ReadinessFilter<Value extends string>({
  options,
  value,
  onChange,
}: ReadinessFilterProps<Value>) {
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);

  const move = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const step = NEXT_KEYS.includes(event.key) ? 1 : PREVIOUS_KEYS.includes(event.key) ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const next = (index + step + options.length) % options.length;
    onChange(options[next].value);
    buttons.current[next]?.focus();
  };

  return (
    <div className="ui-tabs-list" role="radiogroup" aria-label={READINESS_FILTER_LABEL}>
      {options.map((option, index) => {
        const checked = option.value === value;
        return (
          <button
            key={option.value}
            ref={(button) => {
              buttons.current[index] = button;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            tabIndex={checked ? 0 : -1}
            className="ui-tabs-trigger"
            data-state={checked ? ACTIVE : INACTIVE}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => move(event, index)}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
