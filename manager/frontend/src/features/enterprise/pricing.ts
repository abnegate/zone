import { LicenseStatus } from './schemas';
import type { LicenseStatus as LicenseStatusValue } from './types';

export const PRICING_URL = 'https://zone.abnegate.dev/pricing';

const UNLICENSED_STATUSES: ReadonlySet<LicenseStatusValue> = new Set([
  LicenseStatus.Community,
  LicenseStatus.Lapsed,
]);

export function needsLicense(status: LicenseStatusValue): boolean {
  return UNLICENSED_STATUSES.has(status);
}
