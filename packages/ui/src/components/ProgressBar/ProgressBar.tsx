import React, { forwardRef } from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';

const progressBarVariants = cva(
  ['flex flex-col gap-[var(--ui-space-1-5)]'],
  {
    variants: {
      size: {
        sm: '',
        md: '',
        lg: '',
      },
    },
    defaultVariants: {
      size: 'md',
    },
  }
);

const trackVariants = cva(
  [
    'w-full overflow-hidden',
    'bg-[var(--ui-bg-muted)]',
    'rounded-full',
  ],
  {
    variants: {
      size: {
        sm: 'h-1',
        md: 'h-2',
        lg: 'h-3',
      },
    },
    defaultVariants: {
      size: 'md',
    },
  }
);

const fillVariants = cva(
  [
    'h-full',
    'bg-gradient-to-r from-[var(--ui-accent-500)] to-[var(--ui-accent-400)]',
    'rounded-full',
    'transition-all duration-[var(--ui-duration-normal)] ease-out',
  ],
  {
    variants: {
      variant: {
        default: 'from-[var(--ui-accent-500)] to-[var(--ui-accent-400)]',
        success: 'from-[var(--ui-success-500)] to-[var(--ui-success-400)]',
        warning: 'from-[var(--ui-warning-500)] to-[var(--ui-warning-400)]',
        error: 'from-[var(--ui-error-500)] to-[var(--ui-error-400)]',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  }
);

const headerVariants = cva([
  'flex items-center justify-between',
  'text-[var(--ui-text-sm)]',
  'text-[var(--ui-text-secondary)]',
]);

export interface ProgressBarProps
  extends Omit<React.HTMLAttributes<HTMLDivElement>, 'children'>,
    VariantProps<typeof progressBarVariants>,
    VariantProps<typeof fillVariants> {
  value: number;
  max?: number;
  label?: string;
  showPercentage?: boolean;
}

const ProgressBar = forwardRef<HTMLDivElement, ProgressBarProps>(
  (
    {
      value,
      max = 100,
      label,
      showPercentage = true,
      className,
      size,
      variant,
      ...props
    },
    ref
  ) => {
    const percentage = Math.min(100, Math.max(0, Math.round((value / max) * 100)));

    return (
      <div
        ref={ref}
        className={cn(progressBarVariants({ size, className }))}
        role="progressbar"
        aria-valuenow={value}
        aria-valuemin={0}
        aria-valuemax={max}
        aria-label={label}
        {...props}
      >
        {(label || showPercentage) && (
          <div className={cn(headerVariants())}>
            <span>{label}</span>
            {showPercentage && <span>{percentage}%</span>}
          </div>
        )}
        <div className={cn(trackVariants({ size }))}>
          <div
            className={cn(fillVariants({ variant }))}
            style={{ width: `${percentage}%` }}
          />
        </div>
      </div>
    );
  }
);

ProgressBar.displayName = 'ProgressBar';

export { ProgressBar, progressBarVariants, trackVariants, fillVariants };
