import { UpgradeNotice } from '../../components/UpgradeNotice';
import { Permission } from '../../roles/schemas';
import { useUpdateOrganizationPolicy } from '../hooks';
import { UPGRADE_NOTICE } from '../labels';
import { community } from '../license';
import { PolicyScope } from '../source';
import type { OrganizationPolicyResponse } from '../types';
import { PolicyEditor } from './PolicyEditor';
import { PolicyStatus } from './PolicyStatus';

export const ORGANIZATION_READ_ONLY = `Changing the agent policy needs the ${Permission.PoliciesUpdate} permission in this organization.`;

interface OrganizationPolicyProps {
  organizationId: string;
  policy: OrganizationPolicyResponse;
}

export function OrganizationPolicy({ organizationId, policy }: OrganizationPolicyProps) {
  const update = useUpdateOrganizationPolicy(organizationId);

  if (community(policy)) {
    return <UpgradeNotice message={UPGRADE_NOTICE} className="policy-notice" />;
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
