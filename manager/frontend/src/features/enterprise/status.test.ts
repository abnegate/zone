import { describe, expect, it } from 'bun:test';
import {
  ACTIVE,
  COMMUNITY,
  ENFORCEMENT_CLAIM,
  GRACE,
  LAPSED,
  OVER_SEATS,
  UNCOUNTED,
  UNCOUNTED_COMMUNITY,
  UNVERIFIED,
} from './fixtures';
import { LICENSE_REFUSAL_CODES, LicenseRefusalCode, UNVERIFIED_CODES } from './schemas';
import {
  editionLabel,
  expiryLabel,
  licenseDate,
  overSeatNotice,
  refusalMessage,
  seatText,
  statusLabel,
  statusNotice,
  statusTone,
  storesLicense,
  Tone,
  unverifiedReason,
} from './status';

describe('editionLabel', () => {
  it('names each edition', () => {
    expect(editionLabel('community')).toBe('Community');
    expect(editionLabel('team')).toBe('Team');
    expect(editionLabel('enterprise')).toBe('Enterprise');
  });
});

describe('statusLabel and statusTone', () => {
  it('labels and tones each status', () => {
    expect([statusLabel('community'), statusTone('community')]).toEqual([
      'Community',
      Tone.Neutral,
    ]);
    expect([statusLabel('active'), statusTone('active')]).toEqual(['Active', Tone.Success]);
    expect([statusLabel('grace'), statusTone('grace')]).toEqual(['Grace period', Tone.Warning]);
    expect([statusLabel('lapsed'), statusTone('lapsed')]).toEqual(['Lapsed', Tone.Error]);
  });
});

describe('licenseDate', () => {
  it('shows the calendar date in UTC so midnight never slips a day', () => {
    expect(licenseDate('2027-11-01T00:00:00Z')).toBe('Nov 1, 2027');
    expect(licenseDate('2027-12-01T23:59:59Z')).toBe('Dec 1, 2027');
  });

  it('shows a value it cannot read unchanged', () => {
    expect(licenseDate('soon')).toBe('soon');
  });
});

describe('expiryLabel', () => {
  it('says Expires while active and Expired after', () => {
    expect(expiryLabel('active')).toBe('Expires');
    expect(expiryLabel('grace')).toBe('Expired');
    expect(expiryLabel('lapsed')).toBe('Expired');
  });
});

describe('statusNotice', () => {
  it('says nothing for Community or an active license', () => {
    expect(statusNotice(COMMUNITY)).toBeNull();
    expect(statusNotice(ACTIVE)).toBeNull();
  });

  it('says when the license expired and asks for renewal before the grace period ends', () => {
    expect(statusNotice(GRACE)).toBe(
      'This license expired on Nov 1, 2027. Its grace period ends Dec 1, 2027; renew before then.'
    );
  });

  it('says when the license lapsed and asks for a renewed key', () => {
    expect(statusNotice(LAPSED)).toBe(
      'This license lapsed on Dec 1, 2027. Paste a renewed license key to replace it.'
    );
  });

  it('claims no feature stops working, since nothing is gated on the license yet', () => {
    for (const response of [GRACE, LAPSED]) {
      expect(statusNotice(response)).not.toMatch(ENFORCEMENT_CLAIM);
    }
  });
});

describe('seatText', () => {
  it('counts used against licensed seats', () => {
    expect(seatText(ACTIVE.seats)).toBe('12 of 25 seats used this month');
  });

  it('counts used seats alone in Community', () => {
    expect(seatText(COMMUNITY.seats)).toBe('12 seats used this month');
    expect(seatText({ used: 1, licensed: null, over: false })).toBe('1 seat used this month');
  });

  it('names a single licensed seat in the singular', () => {
    expect(seatText({ used: 0, licensed: 1, over: false })).toBe('0 of 1 seat used this month');
  });

  it('says usage is unavailable rather than inventing a count, keeping the licensed seats', () => {
    expect(seatText(UNCOUNTED.seats)).toBe('Seat usage unavailable (25 seats licensed)');
    expect(seatText({ used: null, licensed: 1, over: false })).toBe(
      'Seat usage unavailable (1 seat licensed)'
    );
  });

  it('says usage is unavailable in Community', () => {
    expect(seatText(UNCOUNTED_COMMUNITY.seats)).toBe('Seat usage unavailable');
  });
});

describe('overSeatNotice', () => {
  it('says nothing within the licensed seats or in Community', () => {
    expect(overSeatNotice(ACTIVE.seats)).toBeNull();
    expect(overSeatNotice(COMMUNITY.seats)).toBeNull();
  });

  it('says nothing when usage could not be counted, even if the response claims over', () => {
    expect(overSeatNotice(UNCOUNTED.seats)).toBeNull();
    expect(overSeatNotice({ used: null, licensed: 25, over: true })).toBeNull();
  });

  it('counts the extra people and promises nobody loses access', () => {
    const notice = overSeatNotice(OVER_SEATS.seats);
    expect(notice).toContain('30 people were active this month, 5 more than the 25 seats');
    expect(notice).toContain('Everyone keeps access');
    expect(notice).not.toMatch(/block|lock|suspend|disable/i);
  });
});

describe('refusalMessage', () => {
  it('gives every refusal code its own message', () => {
    const messages = LICENSE_REFUSAL_CODES.map(refusalMessage);
    expect(new Set(messages).size).toBe(LICENSE_REFUSAL_CODES.length);
    for (const message of messages) {
      expect(message.length).toBeGreaterThan(0);
    }
  });
});

describe('unverifiedReason', () => {
  it('explains every way a stored key can stop verifying, each differently', () => {
    const reasons = UNVERIFIED_CODES.map(unverifiedReason);
    for (const reason of reasons) {
      expect(reason.length).toBeGreaterThan(0);
    }
    expect(new Set(reasons).size).toBe(UNVERIFIED_CODES.length);
  });

  it('asks for a renewed key when the signing key is no longer trusted', () => {
    expect(unverifiedReason(LicenseRefusalCode.UnknownKey)).toBe(
      'It was signed with a key this release no longer trusts. Paste a renewed key.'
    );
  });
});

describe('storesLicense', () => {
  it('is true whenever the server holds a license row, verified or not', () => {
    expect(storesLicense(ACTIVE)).toBe(true);
    expect(storesLicense(LAPSED)).toBe(true);
    expect(storesLicense(UNVERIFIED)).toBe(true);
  });

  it('is false when no license is stored', () => {
    expect(storesLicense(COMMUNITY)).toBe(false);
  });
});
