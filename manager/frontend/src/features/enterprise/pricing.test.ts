import { describe, expect, it } from 'bun:test';
import { needsLicense, PRICING_URL } from './pricing';
import { LicenseStatus } from './schemas';

describe('pricing', () => {
  it('points at an https pricing page', () => {
    expect(new URL(PRICING_URL).protocol).toBe('https:');
  });

  it('offers a license to Community and lapsed installs only', () => {
    expect(needsLicense(LicenseStatus.Community)).toBe(true);
    expect(needsLicense(LicenseStatus.Lapsed)).toBe(true);
    expect(needsLicense(LicenseStatus.Active)).toBe(false);
    expect(needsLicense(LicenseStatus.Grace)).toBe(false);
  });
});
