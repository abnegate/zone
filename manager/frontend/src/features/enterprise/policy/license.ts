import { Edition, LicenseStatus } from '../schemas';
import type { Edition as EditionValue, LicenseStatus as LicenseStatusValue } from '../types';

interface Licensed {
  entitled: boolean;
  edition: EditionValue;
  status: LicenseStatusValue;
}

export function community(response: Licensed): boolean {
  return response.edition === Edition.Community;
}

export function lapsed(response: Licensed): boolean {
  return !response.entitled && response.status === LicenseStatus.Lapsed;
}
