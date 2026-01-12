import React, { forwardRef } from 'react';
import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';

const buttonVariants = cva(
  // Base styles
  [
    'inline-flex items-center justify-center gap-[var(--ui-space-1-5)]',
    'font-medium leading-none whitespace-nowrap',
    'border-none rounded-[var(--ui-radius-md)] cursor-pointer',
    'transition-all duration-[var(--ui-duration-fast)] ease-out',
    'select-none',
    'disabled:opacity-50 disabled:cursor-not-allowed disabled:pointer-events-none',
    'active:scale-[0.98]',
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--ui-border-focus)] focus-visible:ring-offset-2',
  ],
  {
    variants: {
      variant: {
        primary: [
          'bg-gradient-to-b from-[var(--ui-accent-500)] to-[var(--ui-accent-600)]',
          'text-white',
          'shadow-[0_1px_2px_rgba(0,0,0,0.1),inset_0_1px_0_rgba(255,255,255,0.1),inset_0_-1px_0_rgba(0,0,0,0.15)]',
          'hover:from-[var(--ui-accent-400)] hover:to-[var(--ui-accent-500)]',
          'hover:shadow-[0_4px_12px_rgba(6,182,212,0.3),inset_0_1px_0_rgba(255,255,255,0.15),inset_0_-1px_0_rgba(0,0,0,0.15)]',
        ],
        secondary: [
          'bg-[var(--ui-bg-surface)]',
          'text-[var(--ui-text-primary)]',
          'border border-[var(--ui-border)]',
          'hover:bg-[var(--ui-bg-hover)]',
          'hover:border-[var(--ui-border-strong)]',
        ],
        danger: [
          'bg-gradient-to-b from-[var(--ui-error-500)] to-[var(--ui-error-600)]',
          'text-white',
          'shadow-[0_1px_2px_rgba(0,0,0,0.1),inset_0_1px_0_rgba(255,255,255,0.1),inset_0_-1px_0_rgba(0,0,0,0.15)]',
          'hover:from-[var(--ui-error-400)] hover:to-[var(--ui-error-500)]',
          'hover:shadow-[0_4px_12px_rgba(244,63,94,0.3),inset_0_1px_0_rgba(255,255,255,0.15),inset_0_-1px_0_rgba(0,0,0,0.15)]',
        ],
        ghost: [
          'bg-transparent',
          'text-[var(--ui-text-secondary)]',
          'hover:bg-[var(--ui-bg-hover)]',
          'hover:text-[var(--ui-text-primary)]',
        ],
        generate: [
          'bg-gradient-to-b from-[var(--ui-secondary-500)] to-[var(--ui-secondary-600)]',
          'text-white',
          'shadow-[0_1px_2px_rgba(0,0,0,0.1),inset_0_1px_0_rgba(255,255,255,0.1),inset_0_-1px_0_rgba(0,0,0,0.15)]',
          'hover:from-[var(--ui-secondary-400)] hover:to-[var(--ui-secondary-500)]',
          'hover:shadow-[0_4px_12px_rgba(16,185,129,0.3),inset_0_1px_0_rgba(255,255,255,0.15),inset_0_-1px_0_rgba(0,0,0,0.15)]',
        ],
      },
      size: {
        sm: 'h-7 px-[var(--ui-space-3)] text-[var(--ui-text-sm)] rounded-[var(--ui-radius-sm)]',
        md: 'h-[34px] px-[var(--ui-space-4)] text-[var(--ui-text-sm)]',
        lg: 'h-10 px-[var(--ui-space-5)] text-[var(--ui-text-base)] rounded-[var(--ui-radius-lg)]',
      },
      tone: {
        default: '',
        success: '',
        warning: '',
        info: '',
      },
    },
    compoundVariants: [
      // Tone modifiers for secondary variant
      {
        variant: 'secondary',
        tone: 'success',
        className: 'text-[var(--ui-success-500)] border-[var(--ui-success-500)] hover:bg-[var(--ui-success-muted)]',
      },
      {
        variant: 'secondary',
        tone: 'warning',
        className: 'text-[var(--ui-warning-500)] border-[var(--ui-warning-500)] hover:bg-[var(--ui-warning-muted)]',
      },
      {
        variant: 'secondary',
        tone: 'info',
        className: 'text-[var(--ui-info-500)] border-[var(--ui-info-500)] hover:bg-[var(--ui-info-muted)]',
      },
    ],
    defaultVariants: {
      variant: 'primary',
      size: 'md',
      tone: 'default',
    },
  }
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
  loading?: boolean;
}

const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  (
    {
      className,
      variant,
      size,
      tone,
      asChild = false,
      loading = false,
      disabled,
      children,
      type = 'button',
      ...props
    },
    ref
  ) => {
    const Comp = asChild ? Slot : 'button';
    return (
      <Comp
        className={cn(buttonVariants({ variant, size, tone, className }))}
        ref={ref}
        disabled={disabled || loading}
        type={type}
        {...props}
      >
        {loading && (
          <span
            className="inline-block w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin"
            aria-hidden="true"
          />
        )}
        {children}
      </Comp>
    );
  }
);

Button.displayName = 'Button';

export { Button, buttonVariants };
export type { VariantProps };
