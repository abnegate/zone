import { Badge } from '@zone/ui';
import type { ReactElement } from 'react';
import type { MessageMetadata } from '../types';
import { automation } from '../utils';

export function AutomationBadge({
  metadata,
}: {
  metadata?: MessageMetadata | null;
}): ReactElement | null {
  const found = automation(metadata);
  if (!found) return null;

  const { kind, detail } = found;

  return (
    <Badge
      variant="neutral"
      className="automation-badge"
      data-testid="automation-badge"
      title={detail ? `${kind.description} ${detail}` : kind.description}
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
        <path d={kind.icon} />
      </svg>
      {kind.label}
      <span className="sr-only">. {kind.description}</span>
    </Badge>
  );
}
