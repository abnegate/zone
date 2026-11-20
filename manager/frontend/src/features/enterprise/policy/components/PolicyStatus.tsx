import { licenseDate } from '../../status';
import { lapsed } from '../license';
import type { OrganizationPolicyResponse, WorkspacePolicyResponse } from '../types';
import { PolicyLapsedNotice } from './PolicyLapsedNotice';

export const UPDATED_LABEL = 'Last changed';

interface PolicyStatusProps {
  policy: OrganizationPolicyResponse | WorkspacePolicyResponse;
  readOnlyHint: string;
}

export function PolicyStatus({ policy, readOnlyHint }: PolicyStatusProps) {
  return (
    <>
      {lapsed(policy) && <PolicyLapsedNotice />}
      {policy.entitled && !policy.editable && <p className="policy-hint">{readOnlyHint}</p>}
      {policy.updated_at && (
        <p className="policy-hint">
          {UPDATED_LABEL} {licenseDate(policy.updated_at)}
        </p>
      )}
    </>
  );
}
