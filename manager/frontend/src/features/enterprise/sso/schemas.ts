import { z } from 'zod';
import { EditionSchema, LicenseStatusSchema } from '../schemas';

export const Protocol = {
  Oidc: 'oidc',
  Saml: 'saml',
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
  MetadataInvalid: 'sso_metadata_invalid',
  DomainRequired: 'sso_domain_required',
  VerifiedEmailUnsupported: 'sso_verified_email_unsupported',
  ProtocolMismatch: 'sso_protocol_mismatch',
} as const;

export const PROTOCOLS = [Protocol.Oidc, Protocol.Saml] as const;

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
  SsoAdminCode.MetadataInvalid,
  SsoAdminCode.DomainRequired,
  SsoAdminCode.VerifiedEmailUnsupported,
  SsoAdminCode.ProtocolMismatch,
] as const;

export const ProtocolSchema = z.enum(PROTOCOLS);

export const SsoAdminCodeSchema = z.enum(SSO_ADMIN_CODES);

export const ProviderResponseSchema = z.object({
  id: z.string(),
  protocol: ProtocolSchema,
  display_name: z.string(),
  issuer: z.string(),
  client_id: z.string().nullable(),
  sso_url: z.string().nullable(),
  metadata_url: z.string().nullable(),
  certificate_fingerprints: z.array(z.string()),
  allowed_domains: z.array(z.string()),
  require_verified_email: z.boolean(),
  enabled: z.boolean(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const ServiceProviderResponseSchema = z.object({
  entity_id: z.string(),
  acs_url: z.string(),
  metadata_url: z.string(),
});

export const SettingsResponseSchema = z.object({
  entitled: z.boolean(),
  edition: EditionSchema,
  status: LicenseStatusSchema,
  required: z.boolean(),
  redirect_uri: z.string(),
  service_provider: ServiceProviderResponseSchema,
  providers: z.array(ProviderResponseSchema),
});

const OIDC_CONNECTION = ['issuer', 'client_id', 'client_secret'] as const;

const SAML_CONNECTION = ['metadata_url', 'metadata'] as const;

export const CreateProviderRequestSchema = z
  .object({
    protocol: ProtocolSchema.optional(),
    display_name: z.string().trim().min(1),
    issuer: z.string().trim().min(1).optional(),
    client_id: z.string().trim().min(1).optional(),
    client_secret: z.string().min(1).optional(),
    metadata_url: z.string().trim().min(1).optional(),
    metadata: z.string().trim().min(1).optional(),
    allowed_domains: z.array(z.string()).optional(),
    require_verified_email: z.boolean().optional(),
    enabled: z.boolean().optional(),
  })
  .superRefine((request, context) => {
    const saml = request.protocol === Protocol.Saml;
    const required = saml ? [] : OIDC_CONNECTION;
    const forbidden = saml ? OIDC_CONNECTION : SAML_CONNECTION;
    for (const field of required) {
      if (request[field] === undefined) {
        context.addIssue({ code: 'custom', path: [field], message: `${field} is required` });
      }
    }
    for (const field of forbidden) {
      if (request[field] !== undefined) {
        context.addIssue({ code: 'custom', path: [field], message: `${field} is not allowed` });
      }
    }
    const sources = SAML_CONNECTION.filter((field) => request[field] !== undefined).length;
    if (saml && sources !== 1) {
      context.addIssue({
        code: 'custom',
        path: ['metadata'],
        message: 'Provide a metadata URL or metadata XML, not both',
      });
    }
  });

export const UpdateProviderRequestSchema = z.object({
  display_name: z.string().trim().min(1).optional(),
  issuer: z.string().trim().min(1).optional(),
  client_id: z.string().trim().min(1).optional(),
  client_secret: z.string().min(1).optional(),
  metadata_url: z.string().trim().min(1).optional(),
  metadata: z.string().trim().min(1).optional(),
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
