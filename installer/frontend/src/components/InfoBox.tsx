import React from 'react';

type InfoBoxVariant = 'info' | 'warning' | 'success';

interface InfoBoxProps {
  variant?: InfoBoxVariant;
  children: React.ReactNode;
}

export function InfoBox({ variant = 'info', children }: InfoBoxProps) {
  return (
    <div className={`info-box ${variant}`}>
      {children}
    </div>
  );
}
