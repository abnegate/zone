import { includes } from '../plans';
import { LicenseStatus, Plan } from '../schemas';
import type { Edition as EditionValue, LicenseStatus as LicenseStatusValue } from '../types';

interface Licensed {
  entitled: boolean;
  edition: EditionValue;
  status: LicenseStatusValue;
}

export function community(response: Licensed): boolean {
  return !includes(response.edition, Plan.Enterprise);
}

export function lapsed(response: Licensed): boolean {
  return (
    !response.entitled &&
    response.status === LicenseStatus.Lapsed &&
    includes(response.edition, Plan.Enterprise)
  );
}
