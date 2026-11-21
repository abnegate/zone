import { COMMUNITY_MEMBERS } from './limits';
import { editionOf } from './plans';
import {
  Edition,
  LicenseRefusalCode,
  LicenseStatus,
  Plan,
  RenewalMode,
  RenewalOutcome,
} from './schemas';
import type { LicenseDetails, LicenseResponse, Members, Plan as PlanValue, Renewal } from './types';

export const ACME_LICENSE: LicenseDetails = {
  id: '6f1c2a9e-3b4d-4e5f-8a7b-9c0d1e2f3a4b',
  plan: Plan.Enterprise,
  customer: 'Acme Ltd',
  seats: 25,
  issued_at: '2026-11-01T00:00:00Z',
  expires_at: '2027-11-01T00:00:00Z',
  grace_ends_at: '2027-12-01T00:00:00Z',
  key_id: '2026-1',
};

export const AUTOMATIC_RENEWAL: Renewal = {
  mode: RenewalMode.Automatic,
  attempted_at: null,
  outcome: null,
};

export function capped(active: number | null, cap = COMMUNITY_MEMBERS): Members {
  return { active, cap, over: active !== null && active > cap };
}

export function uncapped(active: number | null): Members {
  return { active, cap: null, over: false };
}

export function community(used: number): LicenseResponse {
  return {
    edition: Edition.Community,
    status: LicenseStatus.Community,
    license: null,
    seats: { used, licensed: null, over: false },
    members: capped(used),
    unverified: null,
    renewal: null,
  };
}

export const COMMUNITY = community(2);

export const COMMUNITY_AT_CAP = community(COMMUNITY_MEMBERS);

export const COMMUNITY_OVER_CAP = community(5);

export const UNVERIFIED: LicenseResponse = {
  ...COMMUNITY,
  unverified: { code: LicenseRefusalCode.UnknownKey },
};

export function licensed(
  status: Exclude<LicenseResponse['status'], typeof LicenseStatus.Community>,
  used = 12,
  plan: PlanValue = Plan.Enterprise
): LicenseResponse {
  return {
    edition: editionOf(plan),
    status,
    license: { ...ACME_LICENSE, plan },
    seats: { used, licensed: ACME_LICENSE.seats, over: used > ACME_LICENSE.seats },
    members: status === LicenseStatus.Lapsed ? capped(used) : uncapped(used),
    unverified: null,
    renewal: AUTOMATIC_RENEWAL,
  };
}

export const ACTIVE = licensed(LicenseStatus.Active);

export const GRACE = licensed(LicenseStatus.Grace);

export const LAPSED = licensed(LicenseStatus.Lapsed);

export const TEAM = licensed(LicenseStatus.Active, 12, Plan.Team);

export const TEAM_LAPSED = licensed(LicenseStatus.Lapsed, 12, Plan.Team);

export const OVER_SEATS = licensed(LicenseStatus.Active, 30);

export const RENEWED: LicenseResponse = {
  ...ACTIVE,
  renewal: {
    mode: RenewalMode.Automatic,
    attempted_at: '2027-10-20T03:00:00Z',
    outcome: RenewalOutcome.Renewed,
  },
};

export const RENEWAL_UNREACHABLE: LicenseResponse = {
  ...ACTIVE,
  renewal: {
    mode: RenewalMode.Automatic,
    attempted_at: '2027-10-25T03:00:00Z',
    outcome: RenewalOutcome.Unreachable,
  },
};

export const RENEWAL_ENDED: LicenseResponse = {
  ...ACTIVE,
  renewal: {
    mode: RenewalMode.Automatic,
    attempted_at: '2027-10-27T03:00:00Z',
    outcome: RenewalOutcome.Ended,
  },
};

export const RENEWAL_DISABLED: LicenseResponse = {
  ...ACTIVE,
  renewal: { mode: RenewalMode.Disabled, attempted_at: null, outcome: null },
};

export const MANUAL_RENEWAL: LicenseResponse = {
  ...ACTIVE,
  renewal: { mode: RenewalMode.Manual, attempted_at: null, outcome: null },
};

export const UNCOUNTED: LicenseResponse = {
  ...ACTIVE,
  seats: { used: null, licensed: ACME_LICENSE.seats, over: false },
  members: uncapped(null),
};

export const UNCOUNTED_COMMUNITY: LicenseResponse = {
  ...COMMUNITY,
  seats: { used: null, licensed: null, over: false },
  members: capped(null),
};

export const ENFORCEMENT_CLAIM = /configur|stop|sign-in|polic|disable/i;
