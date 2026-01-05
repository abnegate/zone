import React, { forwardRef } from 'react';

export interface SelectOption {
  value: string;
  label: string;
}

export interface SelectProps extends React.ComponentPropsWithoutRef<'select'> {
  label: string;
  options: SelectOption[];
  helpText?: string;
  error?: string;
}

export const Select = forwardRef<HTMLSelectElement, SelectProps>(
  (
    {
      label,
      options,
      helpText,
      error,
      id,
      className = '',
      ...props
    },
    ref
  ) => {
    const selectId = id || label.toLowerCase().replace(/\s+/g, '-');
    const selectClasses = [
      'ui-select',
      error ? 'ui-select--error' : '',
      className,
    ].filter(Boolean).join(' ');

    return (
      <div className="ui-form-field">
        <label className="ui-form-field__label" htmlFor={selectId}>
          {label}
        </label>
        <select
          ref={ref}
          id={selectId}
          className={selectClasses}
          {...props}
        >
          {options.map(option => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        {error && <p className="ui-form-field__error">{error}</p>}
        {helpText && !error && <p className="ui-form-field__help">{helpText}</p>}
      </div>
    );
  }
);

Select.displayName = 'Select';
