import React, { forwardRef } from 'react';
import { Button } from '../Button';

export interface InputProps extends React.ComponentPropsWithoutRef<'input'> {
  label: string;
  helpText?: string;
  error?: string;
  onGenerate?: () => void;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  (
    {
      label,
      helpText,
      error,
      onGenerate,
      id,
      className = '',
      type = 'text',
      ...props
    },
    ref
  ) => {
    const inputId = id || label.toLowerCase().replace(/\s+/g, '-');
    const inputClasses = [
      'ui-input',
      error ? 'ui-input--error' : '',
      className,
    ].filter(Boolean).join(' ');

    const inputElement = (
      <input
        ref={ref}
        id={inputId}
        className={inputClasses}
        type={type}
        {...props}
      />
    );

    return (
      <div className="ui-form-field">
        <label className="ui-form-field__label" htmlFor={inputId}>
          {label}
        </label>
        {onGenerate ? (
          <div className="ui-input-wrapper">
            {inputElement}
            <Button variant="generate" type="button" onClick={onGenerate}>
              Generate
            </Button>
          </div>
        ) : (
          inputElement
        )}
        {error && <p className="ui-form-field__error">{error}</p>}
        {helpText && !error && <p className="ui-form-field__help">{helpText}</p>}
      </div>
    );
  }
);

Input.displayName = 'Input';
