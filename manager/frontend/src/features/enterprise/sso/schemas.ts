import { z } from 'zod';
import { EditionSchema, LicenseStatusSchema } from '../schemas';

export const Protocol = {
  Oidc: 'oidc',
} as const;

export const SsoAdminCode = {
  LicenseRequired: 'license_required',
  ProviderNotFound: 'sso_provider_not_found',
  ProviderRequired: 'sso_provider_required',
  ProviderDuplicate: 'sso_provider_duplicate',
  DisplayNameInvalid: 'sso_display_name_invalid',
  IssuerInvalid: 'sso_issuer_invalid',
  ClientIdInvalid: 'sso_client_id_invalid',
  ClientSecretInvalid: 'sso_client_secret_invalid',
  DomainInvalid: 'sso_domain_invalid',
  DiscoveryFailed: 'sso_discovery_failed',
} as const;

export const PROTOCOLS = [Protocol.Oidc] as const;

export const SSO_ADMIN_CODES = [
  SsoAdminCode.LicenseRequired,
  SsoAdminCode.ProviderNotFound,
  SsoAdminCode.ProviderRequired,
  SsoAdminCode.ProviderDuplicate,
  SsoAdminCode.DisplayNameInvalid,
  SsoAdminCode.IssuerInvalid,
  SsoAdminCode.ClientIdInvalid,
  SsoAdminCode.ClientSecretInvalid,
  SsoAdminCode.DomainInvalid,
  SsoAdminCode.DiscoveryFailed,
] as const;

export const ProtocolSchema = z.enum(PROTOCOLS);

export const SsoAdminCodeSchema = z.enum(SSO_ADMIN_CODES);

export const ProviderResponseSchema = z.object({
  id: z.string(),
  protocol: ProtocolSchema,
  display_name: z.string(),
  issuer: z.string(),
  client_id: z.string(),
  allowed_domains: z.array(z.string()),
  require_verified_email: z.boolean(),
  enabled: z.boolean(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const SettingsResponseSchema = z.object({
  entitled: z.boolean(),
  edition: EditionSchema,
  status: LicenseStatusSchema,
  required: z.boolean(),
  redirect_uri: z.string(),
  providers: z.array(ProviderResponseSchema),
});

export const CreateProviderRequestSchema = z.object({
  display_name: z.string().trim().min(1),
  issuer: z.string().trim().min(1),
  client_id: z.string().trim().min(1),
  client_secret: z.string().min(1),
  allowed_domains: z.array(z.string()).optional(),
  require_verified_email: z.boolean().optional(),
  enabled: z.boolean().optional(),
});

export const UpdateProviderRequestSchema = z.object({
  display_name: z.string().trim().min(1).optional(),
  issuer: z.string().trim().min(1).optional(),
  client_id: z.string().trim().min(1).optional(),
  client_secret: z.string().min(1).optional(),
  allowed_domains: z.array(z.string()).optional(),
  require_verified_email: z.boolean().optional(),
  enabled: z.boolean().optional(),
});

export const EnforcementRequestSchema = z.object({
  required: z.boolean(),
});

export const EnforcementResponseSchema = z.object({
  required: z.boolean(),
});

export const SsoRefusalSchema = z.object({
  error: z.string(),
  code: SsoAdminCodeSchema,
});
