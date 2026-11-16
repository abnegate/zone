import { Edition, LicenseStatus } from './schemas';
import type { LicenseDetails, LicenseResponse, Tier } from './types';

export const ACME_LICENSE: LicenseDetails = {
  id: '6f1c2a9e-3b4d-4e5f-8a7b-9c0d1e2f3a4b',
  customer: 'Acme Ltd',
  tier: Edition.Enterprise,
  seats: 25,
  issued_at: '2026-11-01T00:00:00Z',
  expires_at: '2027-11-01T00:00:00Z',
  grace_ends_at: '2027-12-01T00:00:00Z',
  key_id: '2026-1',
};

export const COMMUNITY: LicenseResponse = {
  edition: Edition.Community,
  status: LicenseStatus.Community,
  license: null,
  seats: { used: 12, licensed: null, over: false },
};

export function licensed(
  status: Exclude<LicenseResponse['status'], typeof LicenseStatus.Community>,
  tier: Tier = Edition.Enterprise,
  used = 12
): LicenseResponse {
  return {
    edition: tier,
    status,
    license: { ...ACME_LICENSE, tier },
    seats: { used, licensed: ACME_LICENSE.seats, over: used > ACME_LICENSE.seats },
  };
}

export const ACTIVE = licensed(LicenseStatus.Active);

export const GRACE = licensed(LicenseStatus.Grace);

export const LAPSED = licensed(LicenseStatus.Lapsed, Edition.Team);

export const OVER_SEATS = licensed(LicenseStatus.Active, Edition.Enterprise, 30);
