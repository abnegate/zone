import React, { forwardRef, useCallback } from 'react';
import * as CheckboxPrimitive from '@radix-ui/react-checkbox';
import { cn } from '../../lib/utils';
import { Label } from '../Label';

export interface CheckboxProps
  extends Omit<
    React.ComponentPropsWithoutRef<typeof CheckboxPrimitive.Root>,
    'checked' | 'defaultChecked' | 'onCheckedChange' | 'onChange'
  > {
  label?: string;
  helpText?: string;
  checked?: boolean;
  defaultChecked?: boolean;
  onChange?: (event: React.ChangeEvent<HTMLInputElement>) => void;
  onCheckedChange?: (checked: boolean) => void;
}

const Checkbox = forwardRef<React.ElementRef<typeof CheckboxPrimitive.Root>, CheckboxProps>(
  (
    {
      label,
      helpText,
      id,
      className,
      checked,
      defaultChecked,
      disabled,
      name,
      value,
      onChange,
      onCheckedChange,
      ...props
    },
    ref
  ) => {
    const checkboxId =
      id || (label ? label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') : undefined);

    const handleCheckedChange = useCallback(
      (nextChecked: boolean | 'indeterminate') => {
        const resolvedChecked = nextChecked === true;
        onCheckedChange?.(resolvedChecked);
        if (onChange) {
          const syntheticEvent = {
            target: { checked: resolvedChecked, name, value },
            currentTarget: { checked: resolvedChecked, name, value },
          } as React.ChangeEvent<HTMLInputElement>;
          onChange(syntheticEvent);
        }
      },
      [name, onChange, onCheckedChange, value]
    );

    return (
      <div className="ui-checkbox-wrapper">
        <div className="ui-checkbox-row">
          <CheckboxPrimitive.Root
            ref={ref}
            id={checkboxId}
            className={cn('ui-checkbox', className)}
            checked={checked}
            defaultChecked={defaultChecked}
            disabled={disabled}
            name={name}
            value={value}
            onCheckedChange={handleCheckedChange}
            {...props}
          >
            <CheckboxPrimitive.Indicator className="ui-checkbox-indicator">
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="3"
                strokeLinecap="round"
                strokeLinejoin="round"
                className="ui-checkbox-icon"
              >
                <polyline points="20 6 9 17 4 12" />
              </svg>
            </CheckboxPrimitive.Indicator>
          </CheckboxPrimitive.Root>
          {label && <Label htmlFor={checkboxId}>{label}</Label>}
        </div>
        {helpText && <p className="ui-checkbox-help-text">{helpText}</p>}
      </div>
    );
  }
);

Checkbox.displayName = 'Checkbox';

export { Checkbox };
