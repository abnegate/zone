import { includes } from '../plans';
import { LicenseStatus, Plan } from '../schemas';
import type { Edition as EditionValue, LicenseStatus as LicenseStatusValue } from '../types';

interface Licensed {
  entitled: boolean;
  edition: EditionValue;
  status: LicenseStatusValue;
}

interface Enforcing extends Licensed {
  enforced: boolean;
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

/**
 * A stored key that no longer verifies reads as Community, yet keeps every feature enforced
 * until an admin replaces or removes it.
 */
export function unverified(response: Enforcing): boolean {
  return response.enforced && !response.entitled && !lapsed(response);
}
