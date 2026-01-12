import React, { forwardRef } from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';

const selectVariants = cva(
  [
    'w-full appearance-none',
    'px-[var(--ui-space-3)] py-[var(--ui-space-2)] pr-[var(--ui-space-8)]',
    'bg-[var(--ui-bg-elevated)]',
    'text-[var(--ui-text-primary)] text-[var(--ui-text-sm)]',
    'border border-[var(--ui-border)] rounded-[var(--ui-radius-md)]',
    'transition-all duration-[var(--ui-duration-fast)] ease-out',
    'focus:outline-none focus:border-[var(--ui-border-focus)] focus:ring-2 focus:ring-[var(--ui-accent-muted)]',
    'disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-[var(--ui-bg-muted)]',
    // Custom dropdown arrow
    'bg-[length:16px_16px] bg-no-repeat bg-[right_var(--ui-space-2)_center]',
    "bg-[url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 24 24' stroke='%2364748b'%3E%3Cpath stroke-linecap='round' stroke-linejoin='round' stroke-width='2' d='M19 9l-7 7-7-7'%3E%3C/path%3E%3C/svg%3E\")]",
  ],
  {
    variants: {
      variant: {
        default: '',
        error: 'border-[var(--ui-error-500)] focus:border-[var(--ui-error-500)] focus:ring-[var(--ui-error-muted)]',
      },
      size: {
        sm: 'h-8 text-[var(--ui-text-xs)]',
        md: 'h-10',
        lg: 'h-12 text-[var(--ui-text-base)]',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'md',
    },
  }
);

const labelVariants = cva([
  'block',
  'mb-[var(--ui-space-1-5)]',
  'text-[var(--ui-text-sm)] font-medium',
  'text-[var(--ui-text-secondary)]',
]);

const helpTextVariants = cva([
  'mt-[var(--ui-space-1)]',
  'text-[var(--ui-text-xs)]',
  'text-[var(--ui-text-muted)]',
]);

const errorTextVariants = cva([
  'mt-[var(--ui-space-1)]',
  'text-[var(--ui-text-xs)]',
  'text-[var(--ui-error-500)]',
]);

export interface SelectOption {
  value: string;
  label: string;
}

export interface SelectProps
  extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, 'size'>,
    VariantProps<typeof selectVariants> {
  label: string;
  options: SelectOption[];
  helpText?: string;
  error?: string;
}

const Select = forwardRef<HTMLSelectElement, SelectProps>(
  (
    {
      label,
      options,
      helpText,
      error,
      id,
      className,
      variant,
      size,
      ...props
    },
    ref
  ) => {
    const selectId = id || label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
    const selectVariant = error ? 'error' : variant;

    return (
      <div className="flex flex-col">
        <label className={cn(labelVariants())} htmlFor={selectId}>
          {label}
        </label>
        <select
          ref={ref}
          id={selectId}
          className={cn(selectVariants({ variant: selectVariant, size, className }))}
          aria-invalid={!!error}
          aria-describedby={error ? `${selectId}-error` : helpText ? `${selectId}-help` : undefined}
          {...props}
        >
          {options.map(option => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        {error && (
          <p id={`${selectId}-error`} className={cn(errorTextVariants())} role="alert">
            {error}
          </p>
        )}
        {helpText && !error && (
          <p id={`${selectId}-help`} className={cn(helpTextVariants())}>
            {helpText}
          </p>
        )}
      </div>
    );
  }
);

Select.displayName = 'Select';

export { Select, selectVariants };
