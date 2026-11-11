import { Badge } from '@zone/ui';
import type { ReactElement } from 'react';
import type { Source } from '../types';

interface SourceStatusBadgeProps {
  source: Source;
}

export function SourceStatusBadge({ source }: SourceStatusBadgeProps): ReactElement {
  if (!source.is_active) {
    return (
      <Badge className="source-status" variant="neutral">
        Inactive
      </Badge>
    );
  }
  if (source.last_error) {
    return (
      <Badge className="source-status" variant="destructive">
        Error
      </Badge>
    );
  }
  if (source.last_verified_at) {
    return (
      <Badge className="source-status" variant="success">
        Verified
      </Badge>
    );
  }
  return (
    <Badge className="source-status" variant="warning">
      Unverified
    </Badge>
  );
}
