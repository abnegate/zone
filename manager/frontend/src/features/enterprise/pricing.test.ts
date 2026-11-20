import { describe, expect, it } from 'bun:test';
import { offersPurchase, PRICING_URL } from './pricing';
import { LICENSE_STATUSES, LicenseStatus } from './schemas';
import type { LicenseStatus as LicenseStatusValue } from './types';

const PURCHASABLE: ReadonlySet<LicenseStatusValue> = new Set([
  LicenseStatus.Community,
  LicenseStatus.Lapsed,
]);

describe('pricing', () => {
  it('points at an https pricing page', () => {
    expect(new URL(PRICING_URL).protocol).toBe('https:');
  });

  for (const status of LICENSE_STATUSES) {
    const offered = PURCHASABLE.has(status);
    it(`${offered ? 'offers' : 'does not offer'} a license to a ${status} install`, () => {
      expect(offersPurchase(status)).toBe(offered);
    });
  }
});
