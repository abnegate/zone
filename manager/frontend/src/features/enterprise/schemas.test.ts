import { describe, expect, it } from 'bun:test';
import {
  ACME_LICENSE,
  ACTIVE,
  COMMUNITY,
  COMMUNITY_AT_CAP,
  COMMUNITY_OVER_CAP,
  GRACE,
  LAPSED,
  MANUAL_RENEWAL,
  OVER_SEATS,
  RENEWAL_DISABLED,
  RENEWAL_UNREACHABLE,
  RENEWED,
  TEAM,
  TEAM_LAPSED,
  UNCOUNTED,
  UNCOUNTED_COMMUNITY,
  UNVERIFIED,
} from './fixtures';
import {
  EDITIONS,
  Edition,
  LICENSE_REFUSAL_CODES,
  LicenseRefusalCode,
  LicenseRefusalSchema,
  LicenseResponseSchema,
  MemberCapCode,
  MembersSchema,
  PLANS,
  Plan,
  PlanSchema,
  RENEWAL_MODES,
  RENEWAL_OUTCOMES,
  RenewalMode,
  SetLicenseRequestSchema,
  UNVERIFIED_CODES,
} from './schemas';

describe('LicenseResponseSchema', () => {
  it('reads the Community response with a null license and no licensed seats', () => {
    expect(LicenseResponseSchema.parse(COMMUNITY)).toEqual(COMMUNITY);
  });

  for (const [name, response] of [
    ['active', ACTIVE],
    ['grace', GRACE],
    ['over-seat', OVER_SEATS],
    ['uncounted', UNCOUNTED],
    ['uncounted Community', UNCOUNTED_COMMUNITY],
    ['renewed', RENEWED],
    ['renewal-unreachable', RENEWAL_UNREACHABLE],
    ['renewal-off', RENEWAL_DISABLED],
    ['manual-renewal', MANUAL_RENEWAL],
    ['Community at the cap', COMMUNITY_AT_CAP],
    ['Community over the cap', COMMUNITY_OVER_CAP],
    ['Team', TEAM],
    ['lapsed Team', TEAM_LAPSED],
  ] as const) {
    it(`reads the ${name} response`, () => {
      expect(LicenseResponseSchema.parse(response)).toEqual(response);
    });
  }

  it('reads a lapsed license that keeps its Enterprise edition', () => {
    const parsed = LicenseResponseSchema.parse(LAPSED);
    expect(parsed.edition).toBe('enterprise');
    expect(parsed.status).toBe('lapsed');
    expect(parsed.license).toEqual(ACME_LICENSE);
  });

  it('reads a Team license with its plan and no member cap', () => {
    const parsed = LicenseResponseSchema.parse(TEAM);
    expect(parsed.edition).toBe(Edition.Team);
    expect(parsed.license?.plan).toBe(Plan.Team);
    expect(parsed.members).toEqual({ active: 12, cap: null, over: false });
  });

  it('rejects an edition the contract does not define', () => {
    expect(LicenseResponseSchema.safeParse({ ...ACTIVE, edition: 'business' }).success).toBe(false);
  });

  it('rejects a license whose plan the contract does not define', () => {
    expect(
      LicenseResponseSchema.safeParse({
        ...ACTIVE,
        license: { ...ACME_LICENSE, plan: 'business' },
      }).success
    ).toBe(false);
  });

  it('rejects a license that leaves its plan out', () => {
    const { plan: _plan, ...missing } = ACME_LICENSE;
    expect(LicenseResponseSchema.safeParse({ ...ACTIVE, license: missing }).success).toBe(false);
  });

  it('rejects a response that leaves the members out', () => {
    const { members: _members, ...missing } = COMMUNITY;
    expect(LicenseResponseSchema.safeParse(missing).success).toBe(false);
  });

  for (const code of UNVERIFIED_CODES) {
    it(`reads a stored key that no longer verifies with ${code} as Community`, () => {
      const response = { ...UNVERIFIED, unverified: { code } };
      expect(LicenseResponseSchema.parse(response)).toEqual(response);
    });
  }

  it('rejects an unverified key that claims the lapsed code, which only a paste can earn', () => {
    expect(
      LicenseResponseSchema.safeParse({
        ...UNVERIFIED,
        unverified: { code: LicenseRefusalCode.Lapsed },
      }).success
    ).toBe(false);
  });

  it('rejects a response that leaves unverified out instead of sending null', () => {
    const { unverified: _unverified, ...missing } = COMMUNITY;
    expect(LicenseResponseSchema.safeParse(missing).success).toBe(false);
  });

  it('rejects a status the contract does not define', () => {
    expect(LicenseResponseSchema.safeParse({ ...ACTIVE, status: 'expired' }).success).toBe(false);
  });

  it('rejects a response that leaves the license out instead of sending null', () => {
    const { license: _license, ...missing } = COMMUNITY;
    expect(LicenseResponseSchema.safeParse(missing).success).toBe(false);
  });

  it('reads seats whose usage could not be counted as null', () => {
    expect(LicenseResponseSchema.parse(UNCOUNTED).seats.used).toBeNull();
  });

  it('rejects seats that leave the used count out instead of sending null', () => {
    expect(
      LicenseResponseSchema.safeParse({ ...ACTIVE, seats: { licensed: 25, over: false } }).success
    ).toBe(false);
  });

  it('rejects a negative used count', () => {
    expect(
      LicenseResponseSchema.safeParse({ ...ACTIVE, seats: { used: -1, licensed: 25, over: false } })
        .success
    ).toBe(false);
  });

  it('rejects seats that leave the licensed count out instead of sending null', () => {
    expect(
      LicenseResponseSchema.safeParse({ ...COMMUNITY, seats: { used: 1, over: false } }).success
    ).toBe(false);
  });
});

describe('plans and editions', () => {
  it('names the three editions and the two plans a license can carry', () => {
    expect(EDITIONS).toEqual(['community', 'team', 'enterprise']);
    expect(PLANS).toEqual(['team', 'enterprise']);
  });

  it('reads each plan and rejects Community, which no license carries', () => {
    for (const plan of PLANS) {
      expect(PlanSchema.parse(plan)).toBe(plan);
    }
    expect(PlanSchema.safeParse(Edition.Community).success).toBe(false);
  });

  it('names the code the server refuses a member with at the cap', () => {
    expect(MemberCapCode).toBe('member_cap_reached');
  });
});

describe('MembersSchema', () => {
  it('reads a capped count and an uncapped one', () => {
    expect(MembersSchema.parse({ active: 3, cap: 3, over: false })).toEqual({
      active: 3,
      cap: 3,
      over: false,
    });
    expect(MembersSchema.parse({ active: 40, cap: null, over: false }).cap).toBeNull();
  });

  it('reads a count the server could not make as null', () => {
    expect(LicenseResponseSchema.parse(UNCOUNTED_COMMUNITY).members.active).toBeNull();
  });

  it('rejects members that leave the active count or the cap out instead of sending null', () => {
    expect(MembersSchema.safeParse({ cap: 3, over: false }).success).toBe(false);
    expect(MembersSchema.safeParse({ active: 1, over: false }).success).toBe(false);
  });

  it('rejects a negative count and a cap of zero', () => {
    expect(MembersSchema.safeParse({ active: -1, cap: 3, over: false }).success).toBe(false);
    expect(MembersSchema.safeParse({ active: 1, cap: 0, over: false }).success).toBe(false);
  });

  it('rejects members that leave over out', () => {
    expect(MembersSchema.safeParse({ active: 1, cap: 3 }).success).toBe(false);
  });
});

describe('RenewalSchema', () => {
  it('names the three renewal modes the server sends', () => {
    expect(RENEWAL_MODES).toEqual(['automatic', 'disabled', 'manual']);
  });

  it('names the six renewal outcomes the server records', () => {
    expect(RENEWAL_OUTCOMES).toEqual([
      'renewed',
      'current',
      'ended',
      'refused',
      'rejected',
      'unreachable',
    ]);
  });

  for (const outcome of RENEWAL_OUTCOMES) {
    it(`reads a renewal attempt that ended ${outcome}`, () => {
      const response = {
        ...ACTIVE,
        renewal: { mode: RenewalMode.Automatic, attempted_at: '2027-10-20T03:00:00Z', outcome },
      };
      expect(LicenseResponseSchema.parse(response)).toEqual(response);
    });
  }

  it('reads an attempt whose stored outcome the server could not name as a null outcome', () => {
    const renewal = {
      mode: RenewalMode.Automatic,
      attempted_at: '2027-10-20T03:00:00Z',
      outcome: null,
    };
    expect(LicenseResponseSchema.parse({ ...ACTIVE, renewal }).renewal).toEqual(renewal);
  });

  it('reads Community with no renewal', () => {
    expect(LicenseResponseSchema.parse(COMMUNITY).renewal).toBeNull();
  });

  it('rejects a response that leaves the renewal out instead of sending null', () => {
    const { renewal: _renewal, ...missing } = ACTIVE;
    expect(LicenseResponseSchema.safeParse(missing).success).toBe(false);
  });

  it('rejects a renewal that leaves the attempt out instead of sending null', () => {
    expect(
      LicenseResponseSchema.safeParse({
        ...ACTIVE,
        renewal: { mode: RenewalMode.Automatic, outcome: null },
      }).success
    ).toBe(false);
  });

  it('rejects a renewal mode the contract does not define', () => {
    expect(
      LicenseResponseSchema.safeParse({
        ...ACTIVE,
        renewal: { mode: 'scheduled', attempted_at: null, outcome: null },
      }).success
    ).toBe(false);
  });

  it('rejects a renewal outcome the contract does not define', () => {
    expect(
      LicenseResponseSchema.safeParse({
        ...ACTIVE,
        renewal: {
          mode: RenewalMode.Automatic,
          attempted_at: '2027-10-20T03:00:00Z',
          outcome: 'expired',
        },
      }).success
    ).toBe(false);
  });
});

describe('SetLicenseRequestSchema', () => {
  it('trims the pasted key', () => {
    expect(SetLicenseRequestSchema.parse({ key: '  abc.def\n' })).toEqual({ key: 'abc.def' });
  });

  it('refuses a key that is only whitespace', () => {
    expect(SetLicenseRequestSchema.safeParse({ key: ' \n\t' }).success).toBe(false);
  });
});

describe('LicenseRefusalSchema', () => {
  it('names the five refusal codes the server sends', () => {
    expect(LICENSE_REFUSAL_CODES).toEqual([
      'license_malformed',
      'license_unknown_key',
      'license_signature_invalid',
      'license_payload_invalid',
      'license_lapsed',
    ]);
  });

  for (const code of LICENSE_REFUSAL_CODES) {
    it(`reads a ${code} refusal`, () => {
      expect(LicenseRefusalSchema.parse({ error: 'Refused', code })).toEqual({
        error: 'Refused',
        code,
      });
    });
  }

  it('rejects a code it does not know', () => {
    expect(LicenseRefusalSchema.safeParse({ error: 'Refused', code: 'nope' }).success).toBe(false);
  });

  it('rejects a bare error without a code', () => {
    expect(LicenseRefusalSchema.safeParse({ error: 'Bad JSON' }).success).toBe(false);
  });
});
