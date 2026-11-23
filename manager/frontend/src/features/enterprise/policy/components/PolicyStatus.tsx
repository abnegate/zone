import { LapsedNotice } from '../../components/LapsedNotice';
import { UnverifiedNotice } from '../../components/UnverifiedNotice';
import { licenseDate } from '../../status';
import { LAPSED_NOTICE, UNVERIFIED_NOTICE } from '../labels';
import { lapsed, unverified } from '../license';
import type { OrganizationPolicyResponse, WorkspacePolicyResponse } from '../types';

export const UPDATED_LABEL = 'Last changed';

interface PolicyStatusProps {
  policy: OrganizationPolicyResponse | WorkspacePolicyResponse;
  readOnlyHint: string;
}

export function PolicyStatus({ policy, readOnlyHint }: PolicyStatusProps) {
  return (
    <>
      {lapsed(policy) && <LapsedNotice message={LAPSED_NOTICE} className="policy-lapsed" />}
      {unverified(policy) && (
        <UnverifiedNotice message={UNVERIFIED_NOTICE} className="policy-lapsed" />
      )}
      {policy.entitled && !policy.editable && <p className="policy-hint">{readOnlyHint}</p>}
      {policy.updated_at && (
        <p className="policy-hint">
          {UPDATED_LABEL} {licenseDate(policy.updated_at)}
        </p>
      )}
    </>
  );
}
