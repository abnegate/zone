import { describe, expect, it } from 'bun:test';
import { EDITIONS, Edition, LicenseStatus } from '../schemas';
import type { Edition as EditionValue, LicenseStatus as LicenseStatusValue } from '../types';
import { community, lapsed } from './license';

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
