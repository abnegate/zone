import React from 'react';
import { cn } from '../../lib/utils';
import { Separator } from '../Separator';

export interface SectionHeaderProps extends React.HTMLAttributes<HTMLDivElement> {
  title: string;
  size?: 'sm' | 'md';
}

const SectionHeader = React.forwardRef<HTMLDivElement, SectionHeaderProps>(
  ({ title, size = 'md', className, ...props }, ref) => {
    const textSize = size === 'sm' ? 'ui-section-title-sm' : undefined;

    return (
      <div ref={ref} className={cn('ui-section-header', className)} {...props}>
        <h3 className={cn('ui-section-title', textSize)}>{title}</h3>
        <Separator className="flex-1" />
      </div>
    );
  }
);

SectionHeader.displayName = 'SectionHeader';

export { SectionHeader };
