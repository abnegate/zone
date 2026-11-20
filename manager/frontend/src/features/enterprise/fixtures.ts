import { Edition, LicenseRefusalCode, LicenseStatus } from './schemas';
import type { LicenseDetails, LicenseResponse } from './types';

export const ACME_LICENSE: LicenseDetails = {
  id: '6f1c2a9e-3b4d-4e5f-8a7b-9c0d1e2f3a4b',
  customer: 'Acme Ltd',
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
  unverified: null,
};

export const UNVERIFIED: LicenseResponse = {
  ...COMMUNITY,
  unverified: { code: LicenseRefusalCode.UnknownKey },
};

export function licensed(
  status: Exclude<LicenseResponse['status'], typeof LicenseStatus.Community>,
  used = 12
): LicenseResponse {
  return {
    edition: Edition.Enterprise,
    status,
    license: ACME_LICENSE,
    seats: { used, licensed: ACME_LICENSE.seats, over: used > ACME_LICENSE.seats },
    unverified: null,
  };
}

export const ACTIVE = licensed(LicenseStatus.Active);

export const GRACE = licensed(LicenseStatus.Grace);

export const LAPSED = licensed(LicenseStatus.Lapsed);

export const OVER_SEATS = licensed(LicenseStatus.Active, 30);

export const UNCOUNTED: LicenseResponse = {
  ...ACTIVE,
  seats: { used: null, licensed: ACME_LICENSE.seats, over: false },
};

export const UNCOUNTED_COMMUNITY: LicenseResponse = {
  ...COMMUNITY,
  seats: { used: null, licensed: null, over: false },
};

export const ENFORCEMENT_CLAIM = /configur|stop|sign-in|polic|disable/i;
