import React, { forwardRef } from 'react';
import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';

const buttonVariants = cva(
  'ui-btn',
  {
    variants: {
      variant: {
        default: 'ui-btn-primary',
        destructive: 'ui-btn-destructive',
        outline: 'ui-btn-outline',
        secondary: 'ui-btn-secondary',
        ghost: 'ui-btn-ghost',
        link: 'ui-btn-link',
      },
      size: {
        default: 'ui-btn-md',
        sm: 'ui-btn-sm',
        lg: 'ui-btn-lg',
        icon: 'ui-btn-icon',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  }
);

type ButtonVariant = VariantProps<typeof buttonVariants>['variant'];
type ButtonSize = VariantProps<typeof buttonVariants>['size'];

const LEGACY_VARIANT_MAP: Record<string, ButtonVariant> = {
  primary: 'default',
  danger: 'destructive',
  generate: 'secondary',
};

const LEGACY_SIZE_MAP: Record<string, ButtonSize> = {
  md: 'default',
};

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    Omit<VariantProps<typeof buttonVariants>, 'variant' | 'size'> {
  variant?: ButtonVariant | 'primary' | 'danger' | 'generate';
  size?: ButtonSize | 'md';
  asChild?: boolean;
  loading?: boolean;
}

const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  (
    {
      className,
      variant,
      size,
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
    const resolvedVariant = (LEGACY_VARIANT_MAP[variant ?? ''] ?? variant) as
      | ButtonVariant
      | undefined;
    const resolvedSize = (LEGACY_SIZE_MAP[size ?? ''] ?? size) as ButtonSize | undefined;

    return (
      <Comp
        ref={ref}
        className={cn(buttonVariants({ variant: resolvedVariant, size: resolvedSize, className }))}
        disabled={disabled || loading}
        type={asChild ? undefined : type}
        {...props}
      >
        {loading && (
          <span className="ui-btn-spinner" aria-hidden="true" />
        )}
        {children}
      </Comp>
    );
  }
);

Button.displayName = 'Button';

export { Button, buttonVariants };
export type { VariantProps };
