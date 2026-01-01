import React from 'react';

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label: string;
  helpText?: string;
  onGenerate?: () => void;
}

export function Input({
  label,
  helpText,
  onGenerate,
  id,
  ...props
}: InputProps) {
  const inputId = id || label.toLowerCase().replace(/\s+/g, '-');

  return (
    <div className="form-field">
      <label htmlFor={inputId}>{label}</label>
      {onGenerate ? (
        <div className="input-with-button">
          <input id={inputId} {...props} />
          <button
            type="button"
            className="btn btn-generate"
            onClick={onGenerate}
          >
            Generate
          </button>
        </div>
      ) : (
        <input id={inputId} {...props} />
      )}
      {helpText && <p className="help-text">{helpText}</p>}
    </div>
  );
}
