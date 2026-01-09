import React, { forwardRef } from 'react';

export interface CheckboxProps extends Omit<React.ComponentPropsWithoutRef<'input'>, 'type'> {
  label: string;
  helpText?: string;
}

export const Checkbox = forwardRef<HTMLInputElement, CheckboxProps>(
  ({ label, helpText, id, className = '', ...props }, ref) => {
    const checkboxId = id || label.toLowerCase().replace(/\s+/g, '-');

    return (
      <div className="ui-form-field ui-form-field--checkbox">
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
        {helpText && <p className="ui-form-field__help">{helpText}</p>}
      </div>
    );
  }
);

Checkbox.displayName = 'Checkbox';
