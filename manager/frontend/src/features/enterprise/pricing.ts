import { LicenseStatus } from './schemas';
import type { LicenseStatus as LicenseStatusValue } from './types';

export const PRICING_URL = 'https://zone.abnegate.dev/pricing';

const OFFERS_PURCHASE: Readonly<Record<LicenseStatusValue, boolean>> = {
  [LicenseStatus.Community]: true,
  [LicenseStatus.Active]: false,
  [LicenseStatus.Grace]: false,
  [LicenseStatus.Lapsed]: true,
};

export function offersPurchase(status: LicenseStatusValue): boolean {
  return OFFERS_PURCHASE[status];
}
