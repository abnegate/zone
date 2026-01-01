import React from 'react';

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label: string;
  helpText?: string;
  error?: string;
  onGenerate?: () => void;
}

export function Input({
  label,
  helpText,
  error,
  onGenerate,
  id,
  className,
  ...props
}: InputProps) {
  const inputId = id || label.toLowerCase().replace(/\s+/g, '-');
  const inputClassName = error ? `${className || ''} input-error`.trim() : className;

  return (
    <div className="form-field">
      <label htmlFor={inputId}>{label}</label>
      {onGenerate ? (
        <div className="input-with-button">
          <input id={inputId} className={inputClassName} {...props} />
          <button
            type="button"
            className="btn btn-generate"
            onClick={onGenerate}
          >
            Generate
          </button>
        </div>
      ) : (
        <input id={inputId} className={inputClassName} {...props} />
      )}
      {error && <p className="field-error">{error}</p>}
      {helpText && !error && <p className="help-text">{helpText}</p>}
    </div>
  );
}
