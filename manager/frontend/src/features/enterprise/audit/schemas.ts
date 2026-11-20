import { z } from 'zod';
import { EditionSchema, LicenseStatusSchema } from '../schemas';

export const AuditScopeKind = {
  Organization: 'organization',
  Server: 'server',
} as const;

export const AuditKind = {
  Webhook: 'webhook',
  S3: 's3',
  Syslog: 'syslog',
} as const;

export const SyslogProtocol = {
  Tls: 'tls',
  Udp: 'udp',
} as const;

export const S3Addressing = {
  Path: 'path',
  VirtualHost: 'virtual_host',
} as const;

export const DeliveryHealth = {
  Healthy: 'healthy',
  Failing: 'failing',
  Paused: 'paused',
  Disabled: 'disabled',
} as const;

export const StreamingPause = {
  Unlicensed: 'unlicensed',
  Edition: 'edition',
  Lapsed: 'lapsed',
  Unverified: 'unverified',
} as const;

export const RetentionWait = {
  Backlog: 'backlog',
  Failing: 'failing',
  Paused: 'paused',
} as const;

export const DeliveryFailure = {
  Connect: 'connect',
  Timeout: 'timeout',
  Tls: 'tls',
  Status: 'status',
  Target: 'target',
  Configuration: 'configuration',
  Unsealable: 'unsealable',
} as const;

export const SecretName = {
  SigningSecret: 'signing_secret',
  AccessKeyId: 'access_key_id',
  SecretAccessKey: 'secret_access_key',
  CaCertificate: 'ca_certificate',
  ClientCertificate: 'client_certificate',
  ClientKey: 'client_key',
} as const;

export const ExportFormat = {
  Ndjson: 'ndjson',
  Csv: 'csv',
} as const;

export const AuditCode = {
  LicenseRequired: 'license_required',
  BodyInvalid: 'audit_body_invalid',
  DestinationNotFound: 'audit_destination_not_found',
  DestinationDuplicate: 'audit_destination_duplicate',
  NameInvalid: 'audit_destination_name_invalid',
  KindInvalid: 'audit_destination_kind_invalid',
  UrlInvalid: 'audit_destination_url_invalid',
  BucketInvalid: 'audit_destination_bucket_invalid',
  RegionInvalid: 'audit_destination_region_invalid',
  PrefixInvalid: 'audit_destination_prefix_invalid',
  HostInvalid: 'audit_destination_host_invalid',
  PortInvalid: 'audit_destination_port_invalid',
  FacilityInvalid: 'audit_destination_facility_invalid',
  HostnameInvalid: 'audit_destination_hostname_invalid',
  SecretInvalid: 'audit_destination_secret_invalid',
  CertificateInvalid: 'audit_destination_certificate_invalid',
  RetentionInvalid: 'audit_retention_invalid',
  ExportRangeInvalid: 'audit_export_range_invalid',
  ExportFormatInvalid: 'audit_export_format_invalid',
  Rebasing: 'audit_rebasing',
} as const;

export const AUDIT_SCOPE_KINDS = [AuditScopeKind.Organization, AuditScopeKind.Server] as const;

export const AUDIT_KINDS = [AuditKind.Webhook, AuditKind.S3, AuditKind.Syslog] as const;

export const SYSLOG_PROTOCOLS = [SyslogProtocol.Tls, SyslogProtocol.Udp] as const;

export const S3_ADDRESSINGS = [S3Addressing.Path, S3Addressing.VirtualHost] as const;

export const DELIVERY_HEALTHS = [
  DeliveryHealth.Healthy,
  DeliveryHealth.Failing,
  DeliveryHealth.Paused,
  DeliveryHealth.Disabled,
] as const;

export const STREAMING_PAUSES = [
  StreamingPause.Unlicensed,
  StreamingPause.Edition,
  StreamingPause.Lapsed,
  StreamingPause.Unverified,
] as const;

export const RETENTION_WAITS = [
  RetentionWait.Backlog,
  RetentionWait.Failing,
  RetentionWait.Paused,
] as const;

export const DELIVERY_FAILURES = [
  DeliveryFailure.Connect,
  DeliveryFailure.Timeout,
  DeliveryFailure.Tls,
  DeliveryFailure.Status,
  DeliveryFailure.Target,
  DeliveryFailure.Configuration,
  DeliveryFailure.Unsealable,
] as const;

export const SECRET_NAMES = [
  SecretName.SigningSecret,
  SecretName.AccessKeyId,
  SecretName.SecretAccessKey,
  SecretName.CaCertificate,
  SecretName.ClientCertificate,
  SecretName.ClientKey,
] as const;

export const EXPORT_FORMATS = [ExportFormat.Ndjson, ExportFormat.Csv] as const;

export const AUDIT_ADMIN_CODES = [
  AuditCode.LicenseRequired,
  AuditCode.BodyInvalid,
  AuditCode.DestinationNotFound,
  AuditCode.DestinationDuplicate,
  AuditCode.NameInvalid,
  AuditCode.KindInvalid,
  AuditCode.UrlInvalid,
  AuditCode.BucketInvalid,
  AuditCode.RegionInvalid,
  AuditCode.PrefixInvalid,
  AuditCode.HostInvalid,
  AuditCode.PortInvalid,
  AuditCode.FacilityInvalid,
  AuditCode.HostnameInvalid,
  AuditCode.SecretInvalid,
  AuditCode.CertificateInvalid,
  AuditCode.RetentionInvalid,
  AuditCode.ExportRangeInvalid,
  AuditCode.ExportFormatInvalid,
  AuditCode.Rebasing,
] as const;

export const KIND_SECRETS = {
  [AuditKind.Webhook]: [SecretName.SigningSecret],
  [AuditKind.S3]: [SecretName.AccessKeyId, SecretName.SecretAccessKey],
  [AuditKind.Syslog]: [
    SecretName.CaCertificate,
    SecretName.ClientCertificate,
    SecretName.ClientKey,
  ],
} as const;

export const REQUIRED_SECRETS = {
  [AuditKind.Webhook]: [SecretName.SigningSecret],
  [AuditKind.S3]: [SecretName.AccessKeyId, SecretName.SecretAccessKey],
  [AuditKind.Syslog]: [],
} as const;

export const SYSLOG_PORTS = {
  [SyslogProtocol.Tls]: 6514,
  [SyslogProtocol.Udp]: 514,
} as const;

export const PORT_MINIMUM = 1;

export const PORT_MAXIMUM = 65_535;

export const FACILITY_MINIMUM = 0;

export const FACILITY_MAXIMUM = 23;

export const DEFAULT_FACILITY = 13;

export const DEFAULT_SYSLOG_HOSTNAME = 'zone';

export const DEFAULT_ADDRESSING = S3Addressing.VirtualHost;

export const RETENTION_DAYS_MINIMUM = 1;

export const RETENTION_DAYS_MAXIMUM = 2_147_483_647;

export const AuditScopeKindSchema = z.enum(AUDIT_SCOPE_KINDS);

export const AuditKindSchema = z.enum(AUDIT_KINDS);

export const SyslogProtocolSchema = z.enum(SYSLOG_PROTOCOLS);

export const S3AddressingSchema = z.enum(S3_ADDRESSINGS);

export const DeliveryHealthSchema = z.enum(DELIVERY_HEALTHS);

export const StreamingPauseSchema = z.enum(STREAMING_PAUSES);

export const RetentionWaitSchema = z.enum(RETENTION_WAITS);

export const DeliveryFailureSchema = z.enum(DELIVERY_FAILURES);

export const SecretNameSchema = z.enum(SECRET_NAMES);

export const ExportFormatSchema = z.enum(EXPORT_FORMATS);

export const AuditCodeSchema = z.enum(AUDIT_ADMIN_CODES);

const count = z.number().int().nonnegative();

const port = z.number().int().min(PORT_MINIMUM).max(PORT_MAXIMUM);

const facility = z.number().int().min(FACILITY_MINIMUM).max(FACILITY_MAXIMUM);

const retentionDays = z.number().int().min(RETENTION_DAYS_MINIMUM).max(RETENTION_DAYS_MAXIMUM);

export const StandingSchema = z.object({
  edition: EditionSchema,
  status: LicenseStatusSchema,
  export: z.boolean(),
  streaming: z.boolean(),
  retention: z.boolean(),
  applies: z.boolean(),
  pause: StreamingPauseSchema.nullable(),
});

export const WebhookSettingsSchema = z.object({
  kind: z.literal(AuditKind.Webhook),
  url: z.string(),
});

export const S3SettingsSchema = z.object({
  kind: z.literal(AuditKind.S3),
  bucket: z.string(),
  region: z.string(),
  endpoint: z.string().nullable(),
  prefix: z.string(),
  addressing: S3AddressingSchema,
});

export const SyslogSettingsSchema = z.object({
  kind: z.literal(AuditKind.Syslog),
  host: z.string(),
  port,
  protocol: SyslogProtocolSchema,
  facility,
  hostname: z.string(),
});

export const SettingsSchema = z.discriminatedUnion('kind', [
  WebhookSettingsSchema,
  S3SettingsSchema,
  SyslogSettingsSchema,
]);

export const WebhookSettingsRequestSchema = z.strictObject({
  kind: z.literal(AuditKind.Webhook),
  url: z.string().trim().min(1),
});

export const S3SettingsRequestSchema = z.strictObject({
  kind: z.literal(AuditKind.S3),
  bucket: z.string().trim().min(1),
  region: z.string().trim().min(1),
  endpoint: z.string().trim().min(1).nullable().optional(),
  prefix: z.string().optional(),
  addressing: S3AddressingSchema.optional(),
});

export const SyslogSettingsRequestSchema = z.strictObject({
  kind: z.literal(AuditKind.Syslog),
  host: z.string().trim().min(1),
  port,
  protocol: SyslogProtocolSchema,
  facility: facility.optional(),
  hostname: z.string().trim().min(1).optional(),
});

export const SettingsRequestSchema = z.discriminatedUnion('kind', [
  WebhookSettingsRequestSchema,
  S3SettingsRequestSchema,
  SyslogSettingsRequestSchema,
]);

export const WebhookSecretsSchema = z.strictObject({
  signing_secret: z.string().min(1).optional(),
});

export const S3SecretsSchema = z.strictObject({
  access_key_id: z.string().min(1).optional(),
  secret_access_key: z.string().min(1).optional(),
});

export const SyslogSecretsSchema = z.strictObject({
  ca_certificate: z.string().optional(),
  client_certificate: z.string().optional(),
  client_key: z.string().optional(),
});

export const SecretsRequestSchema = z.union([
  WebhookSecretsSchema,
  S3SecretsSchema,
  SyslogSecretsSchema,
]);

export const DeliveryResponseSchema = z.object({
  health: DeliveryHealthSchema,
  backlog: count,
  backlog_capped: z.boolean(),
  oldest_pending_at: z.string().nullable(),
  attempts: count,
  next_attempt_at: z.string().nullable(),
  last_success_at: z.string().nullable(),
  last_failure_at: z.string().nullable(),
  last_error_code: DeliveryFailureSchema.nullable(),
  last_error: z.string().nullable(),
  delivered_count: count,
});

export const DestinationResponseSchema = z
  .object({
    id: z.string(),
    name: z.string(),
    kind: AuditKindSchema,
    enabled: z.boolean(),
    settings: SettingsSchema,
    secrets: z.array(SecretNameSchema),
    delivery: DeliveryResponseSchema,
    created_at: z.string(),
    updated_at: z.string(),
  })
  .refine((destination) => destination.settings.kind === destination.kind, {
    message: 'The settings belong to another kind of destination',
    path: ['settings', 'kind'],
  });

export const DestinationsResponseSchema = z.object({
  standing: StandingSchema,
  stalled_since: z.string().nullable(),
  destinations: z.array(DestinationResponseSchema),
});

export const CreateDestinationRequestSchema = z.strictObject({
  name: z.string().trim().min(1),
  enabled: z.boolean().optional(),
  settings: SettingsRequestSchema,
  secrets: SecretsRequestSchema.optional(),
});

export const UpdateDestinationRequestSchema = z.strictObject({
  name: z.string().trim().min(1).optional(),
  enabled: z.boolean().optional(),
  settings: SettingsRequestSchema.optional(),
  secrets: SecretsRequestSchema.optional(),
});

export const TestResponseSchema = z.object({
  delivered: z.boolean(),
  error_code: DeliveryFailureSchema.nullable(),
  error: z.string().nullable(),
});

export const HolderResponseSchema = z.object({
  destination_id: z.string(),
  name: z.string(),
  wait: RetentionWaitSchema,
});

export const RetentionResponseSchema = z.object({
  standing: StandingSchema,
  days: retentionDays.nullable(),
  cutoff: z.string().nullable(),
  expired: count,
  expired_capped: z.boolean(),
  held: count,
  held_capped: z.boolean(),
  holders: z.array(HolderResponseSchema),
  swept_at: z.string().nullable(),
  deleted_count: count,
  updated_at: z.string().nullable(),
});

export const UpdateRetentionRequestSchema = z.strictObject({
  days: retentionDays.nullable(),
});

export const ExportQuerySchema = z.strictObject({
  format: ExportFormatSchema,
  start_date: z.string().trim().min(1),
  end_date: z.string().trim().min(1),
});

export const AuditRefusalSchema = z.object({
  error: z.string(),
  code: AuditCodeSchema,
});
