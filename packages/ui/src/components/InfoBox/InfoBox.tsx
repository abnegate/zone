import React from 'react';
import { Alert } from '../Alert';

export interface InfoBoxProps extends React.HTMLAttributes<HTMLDivElement> {
  variant?: 'default' | 'info' | 'warning' | 'success' | 'error' | 'destructive';
}

const INFOBOX_VARIANT_MAP: Record<string, 'default' | 'destructive'> = {
  default: 'default',
  info: 'default',
  success: 'default',
  warning: 'destructive',
  error: 'destructive',
  destructive: 'destructive',
};

const InfoBox = React.forwardRef<HTMLDivElement, InfoBoxProps>(
  ({ variant = 'default', className, ...props }, ref) => {
    const mappedVariant = INFOBOX_VARIANT_MAP[variant] ?? 'default';
    return <Alert ref={ref} variant={mappedVariant} className={className} {...props} />;
  }
);

InfoBox.displayName = 'InfoBox';

export { InfoBox };
