import React, { forwardRef } from 'react';

export interface ProgressBarProps extends Omit<React.ComponentPropsWithoutRef<'div'>, 'children'> {
  value: number;
  max?: number;
  label?: string;
  showPercentage?: boolean;
  thin?: boolean;
}

export const ProgressBar = forwardRef<HTMLDivElement, ProgressBarProps>(
  (
    {
      value,
      max = 100,
      label,
      showPercentage = true,
      thin = false,
      className = '',
      ...props
    },
    ref
  ) => {
    const percentage = Math.round((value / max) * 100);
    const classes = [
      'ui-progress',
      thin ? 'ui-progress--thin' : '',
      className,
    ].filter(Boolean).join(' ');

    return (
      <div ref={ref} className={classes} {...props}>
        {(label || showPercentage) && (
          <div className="ui-progress__header">
            <span>{label}</span>
            {showPercentage && <span>{percentage}%</span>}
          </div>
        )}
        <div className="ui-progress__track">
          <div
            className="ui-progress__fill"
            style={{ width: `${percentage}%` }}
          />
        </div>
      </div>
    );
  }
);

ProgressBar.displayName = 'ProgressBar';
