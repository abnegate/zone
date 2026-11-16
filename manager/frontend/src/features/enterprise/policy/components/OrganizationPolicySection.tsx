import { Button } from '@zone/ui';
import { useId } from 'react';
import { useOrganizationPolicy, useUpdateOrganizationPolicy } from '../hooks';
import { community } from '../license';
import { PolicyScope } from '../source';
import type { OrganizationPolicyResponse } from '../types';
import { PolicyEditor } from './PolicyEditor';
import { PolicyStatus } from './PolicyStatus';
import { PolicyUpgradeNotice } from './PolicyUpgradeNotice';
import './PolicySection.css';

export const ORGANIZATION_TITLE = 'Agent policy';

export const ORGANIZATION_DESCRIPTION =
  'Choose which providers, models and coding agents this organization can use.';

export const ORGANIZATION_READ_ONLY =
  'Only organization owners and admins can change the agent policy.';

interface OrganizationPolicyProps {
  organizationId: string;
  policy: OrganizationPolicyResponse;
}

function OrganizationPolicy({ organizationId, policy }: OrganizationPolicyProps) {
  const update = useUpdateOrganizationPolicy(organizationId);

  if (community(policy)) {
    return <PolicyUpgradeNotice />;
  }

  return (
    <>
      <PolicyStatus policy={policy} readOnlyHint={ORGANIZATION_READ_ONLY} />
      <PolicyEditor
        key={organizationId}
        scope={PolicyScope.Organization}
        saved={policy.policy}
        editable={policy.editable}
        pending={update.pending}
        refusal={update.refusal}
        error={update.error}
        onSave={(request, onSaved) => update.update(request, onSaved)}
        onReset={update.reset}
      />
    </>
  );
}

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
