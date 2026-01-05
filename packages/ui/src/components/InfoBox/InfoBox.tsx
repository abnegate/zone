import React, { forwardRef } from 'react';

export type InfoBoxVariant = 'info' | 'warning' | 'success' | 'error';

export interface InfoBoxProps extends React.ComponentPropsWithoutRef<'div'> {
  variant?: InfoBoxVariant;
}

export const InfoBox = forwardRef<HTMLDivElement, InfoBoxProps>(
  ({ variant = 'info', children, className = '', ...props }, ref) => {
    const classes = [
      'ui-info-box',
      `ui-info-box--${variant}`,
      className,
    ].filter(Boolean).join(' ');

    return (
      <div ref={ref} className={classes} {...props}>
        {children}
      </div>
    );
  }
);

InfoBox.displayName = 'InfoBox';
