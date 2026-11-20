import { useUpdateOrganizationPolicy } from '../hooks';
import { community } from '../license';
import { PolicyScope } from '../source';
import type { OrganizationPolicyResponse } from '../types';
import { PolicyEditor } from './PolicyEditor';
import { PolicyStatus } from './PolicyStatus';
import { PolicyUpgradeNotice } from './PolicyUpgradeNotice';

export const ORGANIZATION_READ_ONLY =
  'Only organization owners and admins can change the agent policy.';

interface OrganizationPolicyProps {
  organizationId: string;
  policy: OrganizationPolicyResponse;
}

export function OrganizationPolicy({ organizationId, policy }: OrganizationPolicyProps) {
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
