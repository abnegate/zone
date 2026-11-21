import type { z } from 'zod';
import type {
  AuditCodeSchema,
  AuditKindSchema,
  AuditRefusalSchema,
  AuditScopeKind,
  CreateDestinationRequestSchema,
  DeliveryFailureSchema,
  DeliveryHealthSchema,
  DeliveryResponseSchema,
  DestinationResponseSchema,
  DestinationsResponseSchema,
  ExportFormatSchema,
  ExportQuerySchema,
  HolderResponseSchema,
  RetentionResponseSchema,
  RetentionWaitSchema,
  S3AddressingSchema,
  S3SecretsSchema,
  S3SettingsRequestSchema,
  S3SettingsSchema,
  SecretNameSchema,
  SecretsRequestSchema,
  SettingsRequestSchema,
  SettingsSchema,
  StandingSchema,
  StreamingPauseSchema,
  SyslogProtocolSchema,
  SyslogSecretsSchema,
  SyslogSettingsRequestSchema,
  SyslogSettingsSchema,
  TestResponseSchema,
  UpdateDestinationRequestSchema,
  UpdateRetentionRequestSchema,
  WebhookSecretsSchema,
  WebhookSettingsRequestSchema,
  WebhookSettingsSchema,
} from './schemas';

export type AuditKind = z.infer<typeof AuditKindSchema>;
export type SyslogProtocol = z.infer<typeof SyslogProtocolSchema>;
export type S3Addressing = z.infer<typeof S3AddressingSchema>;
export type DeliveryHealth = z.infer<typeof DeliveryHealthSchema>;
export type StreamingPause = z.infer<typeof StreamingPauseSchema>;
export type RetentionWait = z.infer<typeof RetentionWaitSchema>;
export type DeliveryFailure = z.infer<typeof DeliveryFailureSchema>;
export type SecretName = z.infer<typeof SecretNameSchema>;
export type ExportFormat = z.infer<typeof ExportFormatSchema>;
export type AuditCode = z.infer<typeof AuditCodeSchema>;
export type Standing = z.infer<typeof StandingSchema>;
export type WebhookSettings = z.infer<typeof WebhookSettingsSchema>;
export type S3Settings = z.infer<typeof S3SettingsSchema>;
export type SyslogSettings = z.infer<typeof SyslogSettingsSchema>;
export type Settings = z.infer<typeof SettingsSchema>;
export type WebhookSettingsRequest = z.infer<typeof WebhookSettingsRequestSchema>;
export type S3SettingsRequest = z.infer<typeof S3SettingsRequestSchema>;
export type SyslogSettingsRequest = z.infer<typeof SyslogSettingsRequestSchema>;
export type SettingsRequest = z.infer<typeof SettingsRequestSchema>;
export type WebhookSecrets = z.infer<typeof WebhookSecretsSchema>;
export type S3Secrets = z.infer<typeof S3SecretsSchema>;
export type SyslogSecrets = z.infer<typeof SyslogSecretsSchema>;
export type SecretsRequest = z.infer<typeof SecretsRequestSchema>;
export type DeliveryResponse = z.infer<typeof DeliveryResponseSchema>;
export type DestinationResponse = z.infer<typeof DestinationResponseSchema>;
export type DestinationsResponse = z.infer<typeof DestinationsResponseSchema>;
export type CreateDestinationRequest = z.infer<typeof CreateDestinationRequestSchema>;
export type UpdateDestinationRequest = z.infer<typeof UpdateDestinationRequestSchema>;
export type TestResponse = z.infer<typeof TestResponseSchema>;
export type HolderResponse = z.infer<typeof HolderResponseSchema>;
export type RetentionResponse = z.infer<typeof RetentionResponseSchema>;
export type UpdateRetentionRequest = z.infer<typeof UpdateRetentionRequestSchema>;
export type ExportQuery = z.infer<typeof ExportQuerySchema>;
export type AuditRefusalBody = z.infer<typeof AuditRefusalSchema>;

export type AuditScope =
  | { kind: typeof AuditScopeKind.Organization; organizationId: string }
  | { kind: typeof AuditScopeKind.Server };

export interface ExportDownload {
  blob: Blob;
  filename: string;
}
