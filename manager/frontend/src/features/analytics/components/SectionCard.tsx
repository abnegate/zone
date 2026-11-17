import { Card, CardContent, CardDescription, CardHeader, cn } from '@zone/ui';
import { type ReactNode, useId } from 'react';

export interface SectionCardProps {
  title: string;
  description?: string;
  wide?: boolean;
  className?: string;
  children: ReactNode;
}

export function SectionCard({ title, description, wide, className, children }: SectionCardProps) {
  const titleId = useId();
  return (
    <Card
      role="region"
      aria-labelledby={titleId}
      className={cn('analytics-section', wide && 'analytics-section--wide', className)}
    >
      <CardHeader>
        <h2 id={titleId} className="ui-card-title analytics-section-title">
          {title}
        </h2>
        {description && <CardDescription>{description}</CardDescription>}
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  );
}
