import {
  SelectContent,
  SelectItem,
  type SelectOption,
  SelectRoot,
  SelectTrigger,
  SelectUnavailable,
  SelectValue,
  unavailableId,
} from '@zone/ui';
import { type ReactNode, useId } from 'react';

export function HeaderSelect({
  className,
  label,
  value,
  onValueChange,
  options,
  testId,
  title,
  ariaLabel,
  disabled,
  placeholder,
  display,
}: {
  className: string;
  label: string;
  value: string;
  onValueChange: (value: string) => void;
  options: SelectOption[];
  testId: string;
  title: string;
  ariaLabel: string;
  disabled?: boolean;
  placeholder?: string;
  display?: ReactNode;
}) {
  const reasonsId = useId();
  return (
    <SelectRoot
      value={value}
      disabled={disabled}
      onValueChange={(nextValue) => {
        if (nextValue === '') return;
        onValueChange(nextValue);
      }}
    >
      <SelectTrigger
        className={className}
        aria-label={ariaLabel}
        aria-describedby={unavailableId(options, reasonsId)}
        data-testid={testId}
        title={title}
      >
        <span className="chat-header-select-label">{label}</span>
        <SelectValue placeholder={placeholder}>{display}</SelectValue>
      </SelectTrigger>
      <SelectUnavailable id={reasonsId} options={options} />
      <SelectContent>
        {options.map((option) => (
          <SelectItem
            key={option.value}
            value={option.value}
            disabled={option.disabled}
            description={option.description}
          >
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </SelectRoot>
  );
}
