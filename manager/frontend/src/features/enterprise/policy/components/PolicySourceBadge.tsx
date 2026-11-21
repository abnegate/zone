import { SOURCE_LABELS } from '../labels';
import type { PolicySource } from '../source';

interface PolicySourceBadgeProps {
  source: PolicySource;
}

export function PolicySourceBadge({ source }: PolicySourceBadgeProps) {
  return (
    <span className={`policy-badge policy-badge--${source}`} data-source={source}>
      {SOURCE_LABELS[source]}
    </span>
  );
}
