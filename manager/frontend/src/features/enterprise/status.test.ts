import { describe, expect, it } from 'bun:test';
import { ACTIVE, COMMUNITY, GRACE, LAPSED, OVER_SEATS } from './fixtures';
import { LICENSE_REFUSAL_CODES } from './schemas';
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
  Tone,
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

  it('says when the grace period ends', () => {
    expect(statusNotice(GRACE)).toContain('grace period ends Dec 1, 2027');
    expect(statusNotice(GRACE)).toContain('expired on Nov 1, 2027');
  });

  it('says configuring Enterprise features has stopped while sign-in and policies continue', () => {
    const notice = statusNotice(LAPSED);
    expect(notice).toContain('lapsed on Dec 1, 2027');
    expect(notice).toContain('Configuring Enterprise features has stopped');
    expect(notice).toContain('Sign-in and existing policies keep working');
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
});

describe('overSeatNotice', () => {
  it('says nothing within the licensed seats or in Community', () => {
    expect(overSeatNotice(ACTIVE.seats)).toBeNull();
    expect(overSeatNotice(COMMUNITY.seats)).toBeNull();
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
