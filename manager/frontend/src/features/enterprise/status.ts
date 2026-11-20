import { Edition, LicenseRefusalCode, LicenseStatus, RenewalMode, RenewalOutcome } from './schemas';
import type {
  Edition as EditionValue,
  LicenseRefusalCode as LicenseRefusalCodeValue,
  LicenseResponse,
  LicenseStatus as LicenseStatusValue,
  Renewal,
  RenewalMode as RenewalModeValue,
  RenewalOutcome as RenewalOutcomeValue,
  Seats,
  UnverifiedCode,
} from './types';

export const Tone = {
  Neutral: 'neutral',
  Success: 'success',
  Warning: 'warning',
  Error: 'error',
} as const;

export type Tone = (typeof Tone)[keyof typeof Tone];

const EDITION_LABELS: Readonly<Record<EditionValue, string>> = {
  [Edition.Community]: 'Community',
  [Edition.Enterprise]: 'Enterprise',
};

const STATUS_LABELS: Readonly<Record<LicenseStatusValue, string>> = {
  [LicenseStatus.Community]: 'Community',
  [LicenseStatus.Active]: 'Active',
  [LicenseStatus.Grace]: 'Grace period',
  [LicenseStatus.Lapsed]: 'Lapsed',
};

const STATUS_TONES: Readonly<Record<LicenseStatusValue, Tone>> = {
  [LicenseStatus.Community]: Tone.Neutral,
  [LicenseStatus.Active]: Tone.Success,
  [LicenseStatus.Grace]: Tone.Warning,
  [LicenseStatus.Lapsed]: Tone.Error,
};

const REFUSAL_MESSAGES: Readonly<Record<LicenseRefusalCodeValue, string>> = {
  [LicenseRefusalCode.Malformed]:
    "This isn't a complete Zone license key. Paste the whole key exactly as you received it.",
  [LicenseRefusalCode.UnknownKey]:
    'This license was signed with a key this version of Zone does not recognize. Update Zone or ask for a new license key.',
  [LicenseRefusalCode.SignatureInvalid]:
    'This license key failed its signature check, so it may have been changed or cut short. Paste it again or ask for a new one.',
  [LicenseRefusalCode.PayloadInvalid]:
    'This license key is signed but its details are not valid. Ask for a new license key.',
  [LicenseRefusalCode.Lapsed]:
    'This license is past its grace period, so it can no longer be added. Ask for a renewed license key.',
};

const UNVERIFIED_REASONS: Readonly<Record<UnverifiedCode, string>> = {
  [LicenseRefusalCode.Malformed]:
    'The stored key is incomplete or damaged. Paste the key again exactly as you received it.',
  [LicenseRefusalCode.UnknownKey]:
    'It was signed with a key this release no longer trusts. Paste a renewed key.',
  [LicenseRefusalCode.SignatureInvalid]:
    'Its signature no longer checks out, so it may have been changed. Paste the key again or ask for a new one.',
  [LicenseRefusalCode.PayloadInvalid]:
    "It is signed, but this release can't read its details. Ask for a new license key.",
};

const RENEWAL_MODE_LABELS: Readonly<Record<RenewalModeValue, string>> = {
  [RenewalMode.Automatic]: 'Renews automatically',
  [RenewalMode.Disabled]: 'Automatic renewal is off on this server',
  [RenewalMode.Manual]: 'Renew by pasting a new key before it expires',
};

const RENEWAL_OUTCOME_LABELS: Readonly<Record<RenewalOutcomeValue, string>> = {
  [RenewalOutcome.Renewed]: 'Renewed',
  [RenewalOutcome.Current]: 'Up to date',
  [RenewalOutcome.Ended]: 'Subscription ended',
  [RenewalOutcome.Refused]: 'Refused by the license server',
  [RenewalOutcome.Rejected]: 'Renewed key not accepted',
  [RenewalOutcome.Unreachable]: 'License server unreachable',
};

const RENEWAL_OUTCOME_TONES: Readonly<Record<RenewalOutcomeValue, Tone>> = {
  [RenewalOutcome.Renewed]: Tone.Success,
  [RenewalOutcome.Current]: Tone.Success,
  [RenewalOutcome.Ended]: Tone.Warning,
  [RenewalOutcome.Refused]: Tone.Warning,
  [RenewalOutcome.Rejected]: Tone.Warning,
  [RenewalOutcome.Unreachable]: Tone.Warning,
};

const RENEWAL_UNCHECKED = 'Not checked yet';

const SEATS_UNCOUNTED = 'Seat usage unavailable';

const DATE_FORMAT = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeZone: 'UTC' });

export function editionLabel(edition: EditionValue): string {
  return EDITION_LABELS[edition];
}

export function statusLabel(status: LicenseStatusValue): string {
  return STATUS_LABELS[status];
}

export function statusTone(status: LicenseStatusValue): Tone {
  return STATUS_TONES[status];
}

export function refusalMessage(code: LicenseRefusalCodeValue): string {
  return REFUSAL_MESSAGES[code];
}

export function unverifiedReason(code: UnverifiedCode): string {
  return UNVERIFIED_REASONS[code];
}

export function storesLicense(response: LicenseResponse): boolean {
  return response.license !== null || response.unverified !== null;
}

export function licenseDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : DATE_FORMAT.format(date);
}

export function expiryLabel(status: LicenseStatusValue): string {
  return status === LicenseStatus.Active ? 'Expires' : 'Expired';
}

export function statusNotice(response: LicenseResponse): string | null {
  const { license, status } = response;
  if (license === null) {
    return null;
  }
  switch (status) {
    case LicenseStatus.Grace:
      return `This license expired on ${licenseDate(license.expires_at)}. Its grace period ends ${licenseDate(license.grace_ends_at)}; renew before then.`;
    case LicenseStatus.Lapsed:
      return `This license lapsed on ${licenseDate(license.grace_ends_at)}. Paste a renewed license key to replace it.`;
    default:
      return null;
  }
}

export function renewalModeLabel(mode: RenewalModeValue): string {
  return RENEWAL_MODE_LABELS[mode];
}

export function renewalOutcomeLabel(outcome: RenewalOutcomeValue): string {
  return RENEWAL_OUTCOME_LABELS[outcome];
}

export function renewalOutcomeTone(outcome: RenewalOutcomeValue): Tone {
  return RENEWAL_OUTCOME_TONES[outcome];
}

export function showsRenewalAttempt({ mode, attempted_at }: Renewal): boolean {
  return mode === RenewalMode.Automatic || attempted_at !== null;
}

export function renewalAttemptDate({ attempted_at }: Renewal): string {
  return attempted_at === null ? RENEWAL_UNCHECKED : licenseDate(attempted_at);
}

function seatsNoun(count: number): string {
  return count === 1 ? 'seat' : 'seats';
}

export function seatText({ used, licensed }: Seats): string {
  if (used === null) {
    return licensed === null
      ? SEATS_UNCOUNTED
      : `${SEATS_UNCOUNTED} (${licensed} ${seatsNoun(licensed)} licensed)`;
  }
  if (licensed === null) {
    return `${used} ${seatsNoun(used)} used this month`;
  }
  return `${used} of ${licensed} ${seatsNoun(licensed)} used this month`;
}

export function overSeatNotice({ used, licensed, over }: Seats): string | null {
  if (!over || used === null || licensed === null) {
    return null;
  }
  const extra = used - licensed;
  return `${used} people were active this month, ${extra} more than the ${licensed} ${seatsNoun(licensed)} this license covers. Everyone keeps access; add seats to the license to cover them.`;
}
