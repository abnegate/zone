import React, { forwardRef } from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';

const infoBoxVariants = cva(
  [
    'flex items-start gap-[var(--ui-space-3)]',
    'p-[var(--ui-space-4)]',
    'rounded-[var(--ui-radius-lg)]',
    'border',
    'text-[var(--ui-text-sm)]',
  ],
  {
    variants: {
      variant: {
        info: [
          'bg-[var(--ui-info-muted)]',
          'border-[var(--ui-info-500)]',
          'text-[var(--ui-info-600)]',
          '[&_a]:text-[var(--ui-info-600)] [&_a]:underline',
        ],
        warning: [
          'bg-[var(--ui-warning-muted)]',
          'border-[var(--ui-warning-500)]',
          'text-[var(--ui-warning-600)]',
          '[&_a]:text-[var(--ui-warning-600)] [&_a]:underline',
        ],
        success: [
          'bg-[var(--ui-success-muted)]',
          'border-[var(--ui-success-500)]',
          'text-[var(--ui-success-600)]',
          '[&_a]:text-[var(--ui-success-600)] [&_a]:underline',
        ],
        error: [
          'bg-[var(--ui-error-muted)]',
          'border-[var(--ui-error-500)]',
          'text-[var(--ui-error-600)]',
          '[&_a]:text-[var(--ui-error-600)] [&_a]:underline',
        ],
      },
      size: {
        sm: 'p-[var(--ui-space-3)] text-[var(--ui-text-xs)]',
        md: 'p-[var(--ui-space-4)]',
        lg: 'p-[var(--ui-space-5)] text-[var(--ui-text-base)]',
      },
    },
    defaultVariants: {
      variant: 'info',
      size: 'md',
    },
  }
);

export interface InfoBoxProps
  extends React.HTMLAttributes<HTMLDivElement>,
    VariantProps<typeof infoBoxVariants> {}

const InfoBox = forwardRef<HTMLDivElement, InfoBoxProps>(
  ({ variant, size, children, className, ...props }, ref) => {
    return (
      <div
        ref={ref}
        role="alert"
        className={cn(infoBoxVariants({ variant, size, className }))}
        {...props}
      >
        {children}
      </div>
    );
  }
);

InfoBox.displayName = 'InfoBox';

export { InfoBox, infoBoxVariants };
