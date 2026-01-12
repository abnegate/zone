import React, { forwardRef } from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';
import { Button } from '../Button';

const inputVariants = cva(
  [
    'w-full',
    'px-[var(--ui-space-3)] py-[var(--ui-space-2)]',
    'bg-[var(--ui-bg-elevated)]',
    'text-[var(--ui-text-primary)] text-[var(--ui-text-sm)]',
    'border border-[var(--ui-border)] rounded-[var(--ui-radius-md)]',
    'placeholder:text-[var(--ui-text-muted)]',
    'transition-all duration-[var(--ui-duration-fast)] ease-out',
    'focus:outline-none focus:border-[var(--ui-border-focus)] focus:ring-2 focus:ring-[var(--ui-accent-muted)]',
    'disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-[var(--ui-bg-muted)]',
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

export interface InputProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'size'>,
    VariantProps<typeof inputVariants> {
  label: string;
  helpText?: string;
  error?: string;
  onGenerate?: () => void;
}

const Input = forwardRef<HTMLInputElement, InputProps>(
  (
    {
      label,
      helpText,
      error,
      onGenerate,
      id,
      className,
      type = 'text',
      variant,
      size,
      ...props
    },
    ref
  ) => {
    const inputId = id || label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
    const inputVariant = error ? 'error' : variant;

    const inputElement = (
      <input
        ref={ref}
        id={inputId}
        className={cn(inputVariants({ variant: inputVariant, size, className }))}
        type={type}
        aria-invalid={!!error}
        aria-describedby={error ? `${inputId}-error` : helpText ? `${inputId}-help` : undefined}
        {...props}
      />
    );

    return (
      <div className="flex flex-col">
        <label className={cn(labelVariants())} htmlFor={inputId}>
          {label}
        </label>
        {onGenerate ? (
          <div className="flex gap-[var(--ui-space-2)]">
            {inputElement}
            <Button variant="generate" type="button" onClick={onGenerate}>
              Generate
            </Button>
          </div>
        ) : (
          inputElement
        )}
        {error && (
          <p id={`${inputId}-error`} className={cn(errorTextVariants())} role="alert">
            {error}
          </p>
        )}
        {helpText && !error && (
          <p id={`${inputId}-help`} className={cn(helpTextVariants())}>
            {helpText}
          </p>
        )}
      </div>
    );
  }
);

Input.displayName = 'Input';

export { Input, inputVariants };
