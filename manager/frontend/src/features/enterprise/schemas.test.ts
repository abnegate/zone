import { describe, expect, it } from 'bun:test';
import { ACME_LICENSE, ACTIVE, COMMUNITY, GRACE, LAPSED, OVER_SEATS, UNVERIFIED } from './fixtures';
import {
  LICENSE_REFUSAL_CODES,
  LicenseRefusalCode,
  LicenseRefusalSchema,
  LicenseResponseSchema,
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
  ] as const) {
    it(`reads the ${name} response`, () => {
      expect(LicenseResponseSchema.parse(response)).toEqual(response);
    });
  }

  it('reads a lapsed license that keeps its Team edition', () => {
    const parsed = LicenseResponseSchema.parse(LAPSED);
    expect(parsed.edition).toBe('team');
    expect(parsed.status).toBe('lapsed');
    expect(parsed.license?.tier).toBe('team');
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

  it('rejects seats that leave the licensed count out instead of sending null', () => {
    expect(
      LicenseResponseSchema.safeParse({ ...COMMUNITY, seats: { used: 1, over: false } }).success
    ).toBe(false);
  });

  it('rejects a license whose tier is Community', () => {
    expect(
      LicenseResponseSchema.safeParse({
        ...ACTIVE,
        license: { ...ACME_LICENSE, tier: 'community' },
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
