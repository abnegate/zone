import React from 'react';

interface CheckboxProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type'> {
  label: string;
}

export function Checkbox({
  label,
  id,
  ...props
}: CheckboxProps) {
  const checkboxId = id || label.toLowerCase().replace(/\s+/g, '-');

  return (
    <label className="checkbox-wrapper" htmlFor={checkboxId}>
      <input type="checkbox" id={checkboxId} {...props} />
      <span>{label}</span>
    </label>
  );
}
