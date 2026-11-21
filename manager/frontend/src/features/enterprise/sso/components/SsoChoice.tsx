import { useId } from 'react';

interface SsoChoiceProps<T extends string> {
  legend: string;
  value: T;
  choices: readonly { value: T; label: string }[];
  onChange(value: T): void;
}

export function SsoChoice<T extends string>({
  legend,
  value,
  choices,
  onChange,
}: SsoChoiceProps<T>) {
  const name = useId();

  return (
    <fieldset className="sso-choice">
      <legend className="ui-label sso-choice-legend">{legend}</legend>
      <div className="sso-choice-options">
        {choices.map((choice) => (
          <label key={choice.value} className="sso-choice-option">
            <input
              type="radio"
              name={name}
              value={choice.value}
              checked={value === choice.value}
              onChange={() => onChange(choice.value)}
            />
            <span>{choice.label}</span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}
