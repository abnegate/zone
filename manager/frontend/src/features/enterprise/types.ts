import type { z } from 'zod';
import type {
  EditionSchema,
  LicenseDetailsSchema,
  LicenseRefusalSchema,
  LicenseResponseSchema,
  LicenseStatusSchema,
  MembersSchema,
  PlanSchema,
  RenewalSchema,
  SeatsSchema,
  SetLicenseRequestSchema,
  UnverifiedSchema,
} from './schemas';

export type Edition = z.infer<typeof EditionSchema>;
export type Plan = z.infer<typeof PlanSchema>;
export type LicenseStatus = z.infer<typeof LicenseStatusSchema>;
export type LicenseDetails = z.infer<typeof LicenseDetailsSchema>;
export type Seats = z.infer<typeof SeatsSchema>;
export type Members = z.infer<typeof MembersSchema>;
export type LicenseResponse = z.infer<typeof LicenseResponseSchema>;
export type SetLicenseRequest = z.infer<typeof SetLicenseRequestSchema>;
export type LicenseRefusalBody = z.infer<typeof LicenseRefusalSchema>;
export type LicenseRefusalCode = LicenseRefusalBody['code'];
export type Unverified = z.infer<typeof UnverifiedSchema>;
export type UnverifiedCode = Unverified['code'];
export type Renewal = z.infer<typeof RenewalSchema>;
export type RenewalMode = Renewal['mode'];
export type RenewalOutcome = NonNullable<Renewal['outcome']>;
