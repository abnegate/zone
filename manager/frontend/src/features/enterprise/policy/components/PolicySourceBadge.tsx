import { PolicySource } from '../source';

export const SOURCE_LABELS: Readonly<Record<PolicySource, string>> = {
  [PolicySource.Organization]: 'Inherited from organization',
  [PolicySource.Workspace]: 'Set for this workspace',
  [PolicySource.None]: 'Not restricted',
};

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
