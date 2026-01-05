import React, { forwardRef } from 'react';

export interface CheckboxProps extends Omit<React.ComponentPropsWithoutRef<'input'>, 'type'> {
  label: string;
}

export const Checkbox = forwardRef<HTMLInputElement, CheckboxProps>(
  ({ label, id, className = '', ...props }, ref) => {
    const checkboxId = id || label.toLowerCase().replace(/\s+/g, '-');

    return (
      <label className={`ui-checkbox ${className}`.trim()} htmlFor={checkboxId}>
        <input
          ref={ref}
          type="checkbox"
          id={checkboxId}
          className="ui-checkbox__input"
          {...props}
        />
        <span className="ui-checkbox__label">{label}</span>
      </label>
    );
  }
);

Checkbox.displayName = 'Checkbox';
