import { Button } from '@zone/ui';
import { useId } from 'react';
import { useOrganizationPolicy } from '../hooks';
import { OrganizationPolicy } from './OrganizationPolicy';
import './PolicySection.css';

export const ORGANIZATION_TITLE = 'Agent policy';

export const ORGANIZATION_DESCRIPTION =
  'Choose which providers, models and coding agents this organization can use.';

interface OrganizationPolicySectionProps {
  organizationId: string;
}

export function OrganizationPolicySection({ organizationId }: OrganizationPolicySectionProps) {
  const headingId = useId();
  const { policy, loading, error, refresh } = useOrganizationPolicy(organizationId);

  if (loading) {
    return <div className="loading-state">Loading the agent policy...</div>;
  }

  return (
    <section className="settings-section policy-section" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            {ORGANIZATION_TITLE}
          </h2>
          <p className="section-description">{ORGANIZATION_DESCRIPTION}</p>
        </div>
      </div>
      {error && (
        <div className="alert alert-error alert-inline" role="alert">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="sm" onClick={() => void refresh()}>
            Retry
          </Button>
        </div>
      )}
      {policy && <OrganizationPolicy organizationId={organizationId} policy={policy} />}
    </section>
  );
}
