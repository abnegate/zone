import React, { forwardRef } from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';

const checkboxVariants = cva(
  [
    'peer shrink-0',
    'w-[18px] h-[18px]',
    'appearance-none cursor-pointer',
    'bg-[var(--ui-bg-elevated)]',
    'border border-[var(--ui-border)] rounded-[var(--ui-radius-sm)]',
    'transition-all duration-[var(--ui-duration-fast)] ease-out',
    'focus:outline-none focus:ring-2 focus:ring-[var(--ui-accent-muted)] focus:ring-offset-1',
    'checked:bg-[var(--ui-accent-500)] checked:border-[var(--ui-accent-500)]',
    'checked:bg-[url("data:image/svg+xml,%3Csvg xmlns=\'http://www.w3.org/2000/svg\' viewBox=\'0 0 24 24\' fill=\'none\' stroke=\'white\' stroke-width=\'3\' stroke-linecap=\'round\' stroke-linejoin=\'round\'%3E%3Cpolyline points=\'20 6 9 17 4 12\'%3E%3C/polyline%3E%3C/svg%3E")] checked:bg-center checked:bg-no-repeat checked:bg-[length:12px_12px]',
    'disabled:opacity-50 disabled:cursor-not-allowed',
  ],
  {
    variants: {
      size: {
        sm: 'w-4 h-4 checked:bg-[length:10px_10px]',
        md: 'w-[18px] h-[18px]',
        lg: 'w-5 h-5 checked:bg-[length:14px_14px]',
      },
    },
    defaultVariants: {
      size: 'md',
    },
  }
);

const labelVariants = cva([
  'flex items-center gap-[var(--ui-space-2)] cursor-pointer',
  'text-[var(--ui-text-sm)]',
  'text-[var(--ui-text-primary)]',
  'select-none',
]);

const helpTextVariants = cva([
  'mt-[var(--ui-space-1)]',
  'ml-[calc(18px+var(--ui-space-2))]',
  'text-[var(--ui-text-xs)]',
  'text-[var(--ui-text-muted)]',
]);

export interface CheckboxProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type' | 'size'>,
    VariantProps<typeof checkboxVariants> {
  label: string;
  helpText?: string;
}

const Checkbox = forwardRef<HTMLInputElement, CheckboxProps>(
  ({ label, helpText, id, className, size, ...props }, ref) => {
    const checkboxId = id || label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

    return (
      <div className="flex flex-col">
        <label className={cn(labelVariants())} htmlFor={checkboxId}>
          <input
            ref={ref}
            type="checkbox"
            id={checkboxId}
            className={cn(checkboxVariants({ size, className }))}
            {...props}
          />
          <span>{label}</span>
        </label>
        {helpText && (
          <p className={cn(helpTextVariants())}>
            {helpText}
          </p>
        )}
      </div>
    );
  }
);

Checkbox.displayName = 'Checkbox';

export { Checkbox, checkboxVariants };
