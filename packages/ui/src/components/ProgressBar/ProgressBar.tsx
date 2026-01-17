import React from 'react';
import { cn } from '../../lib/utils';
import { Progress } from '../Progress';

export interface ProgressBarProps extends React.HTMLAttributes<HTMLDivElement> {
  value: number;
  max?: number;
  label?: string;
  showPercentage?: boolean;
}

const ProgressBar = React.forwardRef<HTMLDivElement, ProgressBarProps>(
  ({ value, max = 100, label, showPercentage = true, className, ...props }, ref) => {
    const percentage = Math.min(100, Math.max(0, Math.round((value / max) * 100)));

    return (
      <div ref={ref} className={cn('grid gap-2', className)} {...props}>
        {(label || showPercentage) && (
          <div className="flex items-center justify-between text-sm text-muted-foreground">
            <span>{label}</span>
            {showPercentage && <span>{percentage}%</span>}
          </div>
        )}
        <Progress value={percentage} aria-label={label || 'Progress'} />
      </div>
    );
  }
);

ProgressBar.displayName = 'ProgressBar';

export { ProgressBar };
