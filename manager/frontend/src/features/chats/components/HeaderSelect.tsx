import {
  SelectContent,
  SelectItem,
  type SelectOption,
  SelectRoot,
  SelectTrigger,
  SelectValue,
} from '@zone/ui';
import type { ReactNode } from 'react';

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
        data-testid={testId}
        title={title}
      >
        <span className="chat-header-select-label">{label}</span>
        <SelectValue placeholder={placeholder}>{display}</SelectValue>
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </SelectRoot>
  );
}
