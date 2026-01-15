import React, { forwardRef } from 'react';
import { cn } from '../../lib/utils';
import { Label } from '../Label';
import { Button } from '../Button';

export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  helpText?: string;
  error?: string;
  onGenerate?: () => void;
}

const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ label, helpText, error, onGenerate, id, className, type = 'text', ...props }, ref) => {
    const inputId =
      id || (label ? label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') : undefined);
    const describedBy = error ? `${inputId}-error` : helpText ? `${inputId}-help` : undefined;

    const inputElement = (
      <input
        ref={ref}
        id={inputId}
        className={cn('ui-input', error && 'ui-input-error', className)}
        type={type}
        aria-invalid={!!error}
        aria-describedby={describedBy}
        {...props}
      />
    );

    return (
      <div className="ui-input-wrapper">
        {label && <Label htmlFor={inputId}>{label}</Label>}
        {onGenerate ? (
          <div className="ui-input-with-button">
            {inputElement}
            <Button variant="secondary" type="button" onClick={onGenerate}>
              Generate
            </Button>
          </div>
        ) : (
          inputElement
        )}
        {error && (
          <p id={`${inputId}-error`} className="ui-input-error-text" role="alert">
            {error}
          </p>
        )}
        {helpText && !error && (
          <p id={`${inputId}-help`} className="ui-input-help-text">
            {helpText}
          </p>
        )}
      </div>
    );
  }
);

Input.displayName = 'Input';

export { Input };
