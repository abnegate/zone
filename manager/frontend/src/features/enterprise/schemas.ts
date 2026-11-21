import { z } from 'zod';

export const Edition = {
  Community: 'community',
  Enterprise: 'enterprise',
} as const;

export const LicenseStatus = {
  Community: 'community',
  Active: 'active',
  Grace: 'grace',
  Lapsed: 'lapsed',
} as const;

export const LicenseRefusalCode = {
  Malformed: 'license_malformed',
  UnknownKey: 'license_unknown_key',
  SignatureInvalid: 'license_signature_invalid',
  PayloadInvalid: 'license_payload_invalid',
  Lapsed: 'license_lapsed',
} as const;

export const RenewalMode = {
  Automatic: 'automatic',
  Disabled: 'disabled',
  Manual: 'manual',
} as const;

export const RenewalOutcome = {
  Renewed: 'renewed',
  Current: 'current',
  Ended: 'ended',
  Refused: 'refused',
  Rejected: 'rejected',
  Unreachable: 'unreachable',
} as const;

export const EDITIONS = [Edition.Community, Edition.Enterprise] as const;

export const LICENSE_STATUSES = [
  LicenseStatus.Community,
  LicenseStatus.Active,
  LicenseStatus.Grace,
  LicenseStatus.Lapsed,
] as const;

export const LICENSE_REFUSAL_CODES = [
  LicenseRefusalCode.Malformed,
  LicenseRefusalCode.UnknownKey,
  LicenseRefusalCode.SignatureInvalid,
  LicenseRefusalCode.PayloadInvalid,
  LicenseRefusalCode.Lapsed,
] as const;

export const UNVERIFIED_CODES = [
  LicenseRefusalCode.Malformed,
  LicenseRefusalCode.UnknownKey,
  LicenseRefusalCode.SignatureInvalid,
  LicenseRefusalCode.PayloadInvalid,
] as const;

export const RENEWAL_MODES = [
  RenewalMode.Automatic,
  RenewalMode.Disabled,
  RenewalMode.Manual,
] as const;

export const RENEWAL_OUTCOMES = [
  RenewalOutcome.Renewed,
  RenewalOutcome.Current,
  RenewalOutcome.Ended,
  RenewalOutcome.Refused,
  RenewalOutcome.Rejected,
  RenewalOutcome.Unreachable,
] as const;

export const EditionSchema = z.enum(EDITIONS);

export const LicenseStatusSchema = z.enum(LICENSE_STATUSES);

export const LicenseDetailsSchema = z.object({
  id: z.string(),
  customer: z.string(),
  seats: z.number().int().positive(),
  issued_at: z.string(),
  expires_at: z.string(),
  grace_ends_at: z.string(),
  key_id: z.string(),
});

export const SeatsSchema = z.object({
  used: z.number().int().nonnegative().nullable(),
  licensed: z.number().int().positive().nullable(),
  over: z.boolean(),
});

export const UnverifiedSchema = z.object({
  code: z.enum(UNVERIFIED_CODES),
});

export const RenewalSchema = z.object({
  mode: z.enum(RENEWAL_MODES),
  attempted_at: z.string().nullable(),
  outcome: z.enum(RENEWAL_OUTCOMES).nullable(),
});

export const LicenseResponseSchema = z.object({
  edition: EditionSchema,
  status: LicenseStatusSchema,
  license: LicenseDetailsSchema.nullable(),
  seats: SeatsSchema,
  unverified: UnverifiedSchema.nullable(),
  renewal: RenewalSchema.nullable(),
});

export const SetLicenseRequestSchema = z.object({
  key: z.string().trim().min(1, 'License key is required'),
});

export const LicenseRefusalSchema = z.object({
  error: z.string(),
  code: z.enum(LICENSE_REFUSAL_CODES),
});
