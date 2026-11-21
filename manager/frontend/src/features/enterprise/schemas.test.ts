import { describe, expect, it } from 'bun:test';
import {
  ACME_LICENSE,
  ACTIVE,
  COMMUNITY,
  GRACE,
  LAPSED,
  MANUAL_RENEWAL,
  OVER_SEATS,
  RENEWAL_DISABLED,
  RENEWAL_UNREACHABLE,
  RENEWED,
  UNCOUNTED,
  UNCOUNTED_COMMUNITY,
  UNVERIFIED,
} from './fixtures';
import {
  LICENSE_REFUSAL_CODES,
  LicenseRefusalCode,
  LicenseRefusalSchema,
  LicenseResponseSchema,
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

  it('rejects the Team edition, which Zone no longer sells', () => {
    expect(LicenseResponseSchema.safeParse({ ...ACTIVE, edition: 'team' }).success).toBe(false);
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
