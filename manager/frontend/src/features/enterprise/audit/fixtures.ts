import { Edition, LicenseStatus } from '../schemas';
import {
  AuditKind,
  DEFAULT_FACILITY,
  DEFAULT_SYSLOG_HOSTNAME,
  DeliveryFailure,
  DeliveryHealth,
  ExportFormat,
  RetentionWait,
  S3Addressing,
  SecretName,
  StreamingPause,
  SyslogProtocol,
} from './schemas';
import { organizationScope } from './scope';
import type {
  AuditScope,
  DeliveryResponse,
  DestinationResponse,
  DestinationsResponse,
  ExportQuery,
  RetentionResponse,
  Standing,
  TestResponse,
} from './types';

export const ORGANIZATION_ID = '9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d';

export const ORGANIZATION_SCOPE: AuditScope = organizationScope(ORGANIZATION_ID);

export const CREATED_AT = '2026-11-10T09:00:00Z';

export const UPDATED_AT = '2026-11-15T10:30:00Z';

export const NOW = '2026-11-17T12:00:00Z';

export const COMMUNITY_STANDING: Standing = {
  edition: Edition.Community,
  status: LicenseStatus.Community,
  export: false,
  streaming: false,
  retention: false,
  applies: false,
  pause: StreamingPause.Unlicensed,
};

export const ENTERPRISE_STANDING: Standing = {
  edition: Edition.Enterprise,
  status: LicenseStatus.Active,
  export: true,
  streaming: true,
  retention: true,
  applies: true,
  pause: null,
};

export const GRACE_STANDING: Standing = {
  ...ENTERPRISE_STANDING,
  status: LicenseStatus.Grace,
};

export const LAPSED_STANDING: Standing = {
  edition: Edition.Enterprise,
  status: LicenseStatus.Lapsed,
  export: false,
  streaming: false,
  retention: false,
  applies: true,
  pause: StreamingPause.Lapsed,
};

export const TEAM_STANDING: Standing = {
  edition: Edition.Team,
  status: LicenseStatus.Active,
  export: true,
  streaming: false,
  retention: false,
  applies: true,
  pause: StreamingPause.Edition,
};

export const LAPSED_TEAM_STANDING: Standing = {
  ...TEAM_STANDING,
  status: LicenseStatus.Lapsed,
  export: false,
};

export const UNVERIFIED_STANDING: Standing = {
  edition: Edition.Community,
  status: LicenseStatus.Community,
  export: false,
  streaming: false,
  retention: false,
  applies: true,
  pause: StreamingPause.Unverified,
};

const DELIVERED: DeliveryResponse = {
  health: DeliveryHealth.Healthy,
  backlog: 0,
  backlog_capped: false,
  oldest_pending_at: null,
  attempts: 0,
  next_attempt_at: '2026-11-17T11:59:40Z',
  last_success_at: '2026-11-17T11:59:40Z',
  last_failure_at: null,
  last_error_code: null,
  last_error: null,
  delivered_count: 18_204,
};

export const FAILING_WEBHOOK: DestinationResponse = {
  id: '3f2a1b4c-5d6e-4f70-8a9b-0c1d2e3f4a5b',
  name: 'Splunk HEC',
  kind: AuditKind.Webhook,
  enabled: true,
  settings: { kind: AuditKind.Webhook, url: 'https://siem.acme.com/zone/audit' },
  secrets: [SecretName.SigningSecret],
  delivery: {
    health: DeliveryHealth.Failing,
    backlog: 342,
    backlog_capped: false,
    oldest_pending_at: '2026-11-17T09:12:00Z',
    attempts: 6,
    next_attempt_at: '2026-11-17T12:08:00Z',
    last_success_at: '2026-11-17T09:11:58Z',
    last_failure_at: '2026-11-17T11:52:00Z',
    last_error_code: DeliveryFailure.Status,
    last_error: 'HTTP 503',
    delivered_count: 9_120,
  },
  created_at: CREATED_AT,
  updated_at: UPDATED_AT,
};

export const HEALTHY_S3: DestinationResponse = {
  id: '7b8c9d0e-1f2a-4b3c-9d4e-5f6a7b8c9d0e',
  name: 'Audit archive',
  kind: AuditKind.S3,
  enabled: true,
  settings: {
    kind: AuditKind.S3,
    bucket: 'acme-audit',
    region: 'eu-west-1',
    endpoint: null,
    prefix: 'zone/',
    addressing: S3Addressing.VirtualHost,
  },
  secrets: [SecretName.AccessKeyId, SecretName.SecretAccessKey],
  delivery: DELIVERED,
  created_at: CREATED_AT,
  updated_at: CREATED_AT,
};

export const PAUSED_SYSLOG: DestinationResponse = {
  id: '5c6d7e8f-9a0b-4c1d-8e2f-3a4b5c6d7e8f',
  name: 'Graylog',
  kind: AuditKind.Syslog,
  enabled: true,
  settings: {
    kind: AuditKind.Syslog,
    host: 'graylog.internal',
    port: 6514,
    protocol: SyslogProtocol.Tls,
    facility: DEFAULT_FACILITY,
    hostname: DEFAULT_SYSLOG_HOSTNAME,
  },
  secrets: [SecretName.CaCertificate],
  delivery: {
    ...DELIVERED,
    health: DeliveryHealth.Paused,
    next_attempt_at: null,
    backlog: 57,
    oldest_pending_at: '2026-11-16T08:00:00Z',
    last_success_at: '2026-11-16T07:59:00Z',
    delivered_count: 4_310,
  },
  created_at: CREATED_AT,
  updated_at: CREATED_AT,
};

export const PLAIN_HTTP_WEBHOOK: DestinationResponse = {
  id: '1f2e3d4c-5b6a-4978-8a6b-5c4d3e2f1a0b',
  name: 'LAN collector',
  kind: AuditKind.Webhook,
  enabled: true,
  settings: { kind: AuditKind.Webhook, url: 'http://splunk:8088/services/collector' },
  secrets: [SecretName.SigningSecret],
  delivery: DELIVERED,
  created_at: CREATED_AT,
  updated_at: CREATED_AT,
};

export const DISABLED_UDP_SYSLOG: DestinationResponse = {
  id: '2a3b4c5d-6e7f-4a8b-9c0d-1e2f3a4b5c6d',
  name: 'Legacy syslog',
  kind: AuditKind.Syslog,
  enabled: false,
  settings: {
    kind: AuditKind.Syslog,
    host: '10.0.0.5',
    port: 514,
    protocol: SyslogProtocol.Udp,
    facility: 16,
    hostname: 'zone-eu',
  },
  secrets: [],
  delivery: {
    ...DELIVERED,
    health: DeliveryHealth.Disabled,
    next_attempt_at: null,
    backlog: 0,
    last_success_at: '2026-11-01T00:00:00Z',
    delivered_count: 120,
  },
  created_at: CREATED_AT,
  updated_at: UPDATED_AT,
};

export const COMMUNITY_DESTINATIONS: DestinationsResponse = {
  standing: COMMUNITY_STANDING,
  stalled_since: null,
  destinations: [],
};

export const ENTERPRISE_DESTINATIONS: DestinationsResponse = {
  standing: ENTERPRISE_STANDING,
  stalled_since: null,
  destinations: [FAILING_WEBHOOK, HEALTHY_S3, PLAIN_HTTP_WEBHOOK, DISABLED_UDP_SYSLOG],
};

export const ENTERPRISE_EMPTY_DESTINATIONS: DestinationsResponse = {
  standing: ENTERPRISE_STANDING,
  stalled_since: null,
  destinations: [],
};

export const PAUSED_S3: DestinationResponse = { ...HEALTHY_S3, delivery: PAUSED_SYSLOG.delivery };

export const LAPSED_DESTINATIONS: DestinationsResponse = {
  standing: LAPSED_STANDING,
  stalled_since: null,
  destinations: [PAUSED_SYSLOG, PAUSED_S3],
};

export const TEAM_DESTINATIONS: DestinationsResponse = {
  standing: TEAM_STANDING,
  stalled_since: null,
  destinations: [PAUSED_SYSLOG],
};

export const LAPSED_TEAM_DESTINATIONS: DestinationsResponse = {
  ...TEAM_DESTINATIONS,
  standing: LAPSED_TEAM_STANDING,
};

export const STALLED_DESTINATIONS: DestinationsResponse = {
  standing: ENTERPRISE_STANDING,
  stalled_since: '2026-11-17T11:40:00Z',
  destinations: [HEALTHY_S3],
};

export const SERVER_SYSLOG: DestinationResponse = {
  ...PAUSED_SYSLOG,
  id: '8d9e0f1a-2b3c-4d5e-8f6a-7b8c9d0e1f2a',
  name: 'Server SIEM',
  delivery: DELIVERED,
};

export const SERVER_DESTINATIONS: DestinationsResponse = {
  standing: ENTERPRISE_STANDING,
  stalled_since: null,
  destinations: [SERVER_SYSLOG],
};

export const COMMUNITY_RETENTION: RetentionResponse = {
  standing: COMMUNITY_STANDING,
  days: null,
  cutoff: null,
  expired: 0,
  expired_capped: false,
  held: 0,
  held_capped: false,
  holders: [],
  swept_at: null,
  deleted_count: 0,
  updated_at: null,
};

export const ENTERPRISE_RETENTION: RetentionResponse = {
  standing: ENTERPRISE_STANDING,
  days: 90,
  cutoff: '2026-08-19T12:00:00Z',
  expired: 0,
  expired_capped: false,
  held: 0,
  held_capped: false,
  holders: [],
  swept_at: '2026-11-17T11:00:00Z',
  deleted_count: 1_204,
  updated_at: UPDATED_AT,
};

export const FOREVER_RETENTION: RetentionResponse = {
  ...COMMUNITY_RETENTION,
  standing: ENTERPRISE_STANDING,
};

export const WAITING_RETENTION: RetentionResponse = {
  ...ENTERPRISE_RETENTION,
  expired: 412,
  held: 342,
  holders: [
    {
      destination_id: FAILING_WEBHOOK.id,
      name: FAILING_WEBHOOK.name,
      wait: RetentionWait.Failing,
    },
  ],
  deleted_count: 70,
};

export const CAPPED_RETENTION: RetentionResponse = {
  ...WAITING_RETENTION,
  expired: 10_000,
  expired_capped: true,
  held: 10_000,
  held_capped: true,
};

export const LAPSED_RETENTION: RetentionResponse = {
  ...ENTERPRISE_RETENTION,
  standing: LAPSED_STANDING,
  held: PAUSED_SYSLOG.delivery.backlog + PAUSED_S3.delivery.backlog,
  expired: PAUSED_SYSLOG.delivery.backlog + PAUSED_S3.delivery.backlog,
  holders: LAPSED_DESTINATIONS.destinations.map((destination) => ({
    destination_id: destination.id,
    name: destination.name,
    wait: RetentionWait.Paused,
  })),
  deleted_count: 0,
};

export const COMMUNITY_SAVED_RETENTION: RetentionResponse = {
  ...ENTERPRISE_RETENTION,
  standing: COMMUNITY_STANDING,
};

export const TEAM_RETENTION: RetentionResponse = {
  ...COMMUNITY_RETENTION,
  standing: TEAM_STANDING,
};

export const LAPSED_TEAM_RETENTION: RetentionResponse = {
  ...TEAM_RETENTION,
  standing: LAPSED_TEAM_STANDING,
};

export const SERVER_RETENTION: RetentionResponse = {
  ...ENTERPRISE_RETENTION,
  days: 365,
  cutoff: '2025-11-17T12:00:00Z',
  deleted_count: 0,
};

export const DELIVERED_TEST: TestResponse = {
  delivered: true,
  error_code: null,
  error: null,
};

export const FAILED_TEST: TestResponse = {
  delivered: false,
  error_code: DeliveryFailure.Status,
  error: 'HTTP 500',
};

export const EXPORT_QUERY: ExportQuery = {
  format: ExportFormat.Ndjson,
  start_date: '2026-11-01',
  end_date: '2026-11-15',
};

export const EXPORT_FILENAME = `audit-${ORGANIZATION_ID}-2026-11-01-2026-11-15.ndjson`;

export const SERVER_EXPORT_FILENAME = 'audit-server-2026-11-01-2026-11-15.ndjson';
