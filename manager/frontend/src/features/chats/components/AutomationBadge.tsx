import { Badge } from '@zone/ui';
import type { ReactElement } from 'react';
import type { MessageMetadata } from '../types';
import { automationSource } from '../utils/automation';

export function AutomationBadge({
  metadata,
}: {
  metadata?: MessageMetadata | null;
}): ReactElement | null {
  const source = automationSource(metadata);
  if (!source) return null;

  const reminder = metadata?.reminder_id?.trim();
  const title = reminder ? `${source.description} Reminder ${reminder}.` : source.description;

  return (
    <Badge
      variant="neutral"
      className="automation-badge"
      data-testid="automation-badge"
      title={title}
    >
      <svg
        className="automation-badge-icon"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d={source.icon} />
      </svg>
      {source.label}
      <span className="sr-only"> — {source.description}</span>
    </Badge>
  );
}
