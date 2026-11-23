import { describe, expect, it } from 'bun:test';
import { EDITIONS, Edition, LicenseStatus } from '../schemas';
import type { Edition as EditionValue, LicenseStatus as LicenseStatusValue } from '../types';
import { community, lapsed, unverified } from './license';

function response(edition: EditionValue, status: LicenseStatusValue, entitled = false) {
  return { edition, status, entitled };
}

describe('community', () => {
  it('treats every edition without Enterprise as Community for policy', () => {
    expect(community(response(Edition.Community, LicenseStatus.Community))).toBe(true);
    expect(community(response(Edition.Team, LicenseStatus.Active))).toBe(true);
    expect(community(response(Edition.Team, LicenseStatus.Lapsed))).toBe(true);
  });

  it('keeps Enterprise out of Community whatever its status', () => {
    for (const status of [LicenseStatus.Active, LicenseStatus.Grace, LicenseStatus.Lapsed]) {
      expect(community(response(Edition.Enterprise, status))).toBe(false);
    }
  });
});

describe('lapsed', () => {
  it('is true only for an unentitled, lapsed Enterprise license', () => {
    expect(lapsed(response(Edition.Enterprise, LicenseStatus.Lapsed))).toBe(true);
    expect(lapsed(response(Edition.Enterprise, LicenseStatus.Lapsed, true))).toBe(false);
    expect(lapsed(response(Edition.Enterprise, LicenseStatus.Active))).toBe(false);
  });

  it('never calls a Team or Community server lapsed, since neither ever had policy', () => {
    for (const edition of EDITIONS.filter((value) => value !== Edition.Enterprise)) {
      expect(lapsed(response(edition, LicenseStatus.Lapsed))).toBe(false);
    }
  });
});

describe('unverified', () => {
  const enforcing = (
    edition: EditionValue,
    status: LicenseStatusValue,
    entitled: boolean,
    enforced: boolean
  ) => ({ ...response(edition, status, entitled), enforced });

  it('is true for a stored key that no longer verifies, which reads as Community yet enforces', () => {
    expect(unverified(enforcing(Edition.Community, LicenseStatus.Community, false, true))).toBe(
      true
    );
  });

  it('is false for a lapsed, entitled or unenforced license', () => {
    expect(unverified(enforcing(Edition.Enterprise, LicenseStatus.Lapsed, false, true))).toBe(
      false
    );
    expect(unverified(enforcing(Edition.Enterprise, LicenseStatus.Active, true, true))).toBe(false);
    expect(unverified(enforcing(Edition.Enterprise, LicenseStatus.Grace, true, true))).toBe(false);
    expect(unverified(enforcing(Edition.Community, LicenseStatus.Community, false, false))).toBe(
      false
    );
    expect(unverified(enforcing(Edition.Team, LicenseStatus.Active, false, false))).toBe(false);
  });
});
