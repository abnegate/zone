import React from 'react';
import { cn } from '../../lib/utils';
import { Separator } from '../Separator';

export interface SectionHeaderProps extends React.HTMLAttributes<HTMLDivElement> {
  title: string;
  size?: 'sm' | 'md';
}

const SectionHeader = React.forwardRef<HTMLDivElement, SectionHeaderProps>(
  ({ title, size = 'md', className, ...props }, ref) => {
    const textSize = size === 'sm' ? 'text-sm' : 'text-base';

    return (
      <div ref={ref} className={cn('flex items-center gap-3', className)} {...props}>
        <h3 className={cn('font-display font-semibold text-foreground', textSize)}>{title}</h3>
        <Separator className="flex-1" />
      </div>
    );
  }
);

SectionHeader.displayName = 'SectionHeader';

export { SectionHeader };
