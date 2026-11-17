import { describe, expect, it } from 'bun:test';
import {
  COMMUNITY_RETENTION,
  ENTERPRISE_DESTINATIONS,
  FAILED_TEST,
  FAILING_WEBHOOK,
  HEALTHY_S3,
  LAPSED_DESTINATIONS,
  LAPSED_RETENTION,
  PAUSED_SYSLOG,
  SERVER_DESTINATIONS,
  STALLED_DESTINATIONS,
  TEAM_DESTINATIONS,
  WAITING_RETENTION,
} from './fixtures';
import {
  AUDIT_ADMIN_CODES,
  AUDIT_KINDS,
  AuditKind,
  CreateDestinationRequestSchema,
  DELIVERY_FAILURES,
  DELIVERY_HEALTHS,
  DestinationResponseSchema,
  DestinationsResponseSchema,
  EXPORT_FORMATS,
  ExportQuerySchema,
  KIND_SECRETS,
  REQUIRED_SECRETS,
  RETENTION_DAYS_MAXIMUM,
  RETENTION_WAITS,
  RetentionResponseSchema,
  S3_ADDRESSINGS,
  SECRET_NAMES,
  SettingsSchema,
  STREAMING_PAUSES,
  StandingSchema,
  SYSLOG_PROTOCOLS,
  SyslogProtocol,
  TestResponseSchema,
  UpdateDestinationRequestSchema,
  UpdateRetentionRequestSchema,
} from './schemas';

describe('audit vocabulary', () => {
  it('names every value as the server serialises it', () => {
    expect(AUDIT_KINDS).toEqual(['webhook', 's3', 'syslog']);
    expect(SYSLOG_PROTOCOLS).toEqual(['tls', 'udp']);
    expect(S3_ADDRESSINGS).toEqual(['path', 'virtual_host']);
    expect(DELIVERY_HEALTHS).toEqual(['healthy', 'failing', 'paused', 'disabled']);
    expect(STREAMING_PAUSES).toEqual(['unlicensed', 'edition', 'lapsed', 'unverified']);
    expect(RETENTION_WAITS).toEqual(['backlog', 'failing', 'paused']);
    expect(DELIVERY_FAILURES).toEqual([
      'connect',
      'timeout',
      'tls',
      'status',
      'target',
      'configuration',
      'unsealable',
    ]);
    expect(SECRET_NAMES).toEqual([
      'signing_secret',
      'access_key_id',
      'secret_access_key',
      'ca_certificate',
      'client_certificate',
      'client_key',
    ]);
    expect(EXPORT_FORMATS).toEqual(['ndjson', 'csv']);
  });

  it('lists the license code, the body code and every audit code once', () => {
    expect(AUDIT_ADMIN_CODES).toContain('license_required');
    expect(AUDIT_ADMIN_CODES).toContain('audit_body_invalid');
    expect(new Set(AUDIT_ADMIN_CODES).size).toBe(AUDIT_ADMIN_CODES.length);
    expect(AUDIT_ADMIN_CODES).toHaveLength(19);
    for (const code of AUDIT_ADMIN_CODES) {
      expect(code === 'license_required' || code.startsWith('audit_'), code).toBe(true);
    }
  });

  it('gives every secret to exactly one kind, and requires only secrets of that kind', () => {
    const owned = AUDIT_KINDS.flatMap((kind) => KIND_SECRETS[kind]);
    expect([...owned].sort()).toEqual([...SECRET_NAMES].sort());
    for (const kind of AUDIT_KINDS) {
      for (const name of REQUIRED_SECRETS[kind]) {
        expect(KIND_SECRETS[kind] as readonly string[]).toContain(name);
      }
    }
  });
});

describe('StandingSchema', () => {
  it('requires a pause, null when streaming runs', () => {
    const { pause: _, ...missing } = ENTERPRISE_DESTINATIONS.standing;
    expect(StandingSchema.safeParse(missing).success).toBe(false);
    expect(StandingSchema.safeParse({ ...missing, pause: 'expired' }).success).toBe(false);
  });
});

describe('destination responses', () => {
  it('parses every fixture state', () => {
    for (const response of [
      ENTERPRISE_DESTINATIONS,
      LAPSED_DESTINATIONS,
      TEAM_DESTINATIONS,
      STALLED_DESTINATIONS,
      SERVER_DESTINATIONS,
    ]) {
      expect(DestinationsResponseSchema.parse(response)).toEqual(response);
    }
  });

  it('requires stalled_since, null when nothing is stalled', () => {
    const { stalled_since: _, ...missing } = ENTERPRISE_DESTINATIONS;
    expect(DestinationsResponseSchema.safeParse(missing).success).toBe(false);
  });

  it('reads settings by their kind', () => {
    expect(SettingsSchema.parse(HEALTHY_S3.settings)).toEqual(HEALTHY_S3.settings);
    expect(SettingsSchema.parse(PAUSED_SYSLOG.settings)).toEqual(PAUSED_SYSLOG.settings);
    expect(
      SettingsSchema.safeParse({ kind: AuditKind.Webhook, bucket: 'acme-audit' }).success
    ).toBe(false);
  });

  it('refuses settings of another kind than the destination', () => {
    const mismatched = { ...FAILING_WEBHOOK, settings: HEALTHY_S3.settings };
    expect(DestinationResponseSchema.safeParse(mismatched).success).toBe(false);
  });

  it('refuses a syslog facility above 23 and an unknown secret name', () => {
    const facility = { ...PAUSED_SYSLOG, settings: { ...PAUSED_SYSLOG.settings, facility: 24 } };
    expect(DestinationResponseSchema.safeParse(facility).success).toBe(false);
    const secret = { ...FAILING_WEBHOOK, secrets: ['password'] };
    expect(DestinationResponseSchema.safeParse(secret).success).toBe(false);
  });

  it('parses a test result', () => {
    expect(TestResponseSchema.parse(FAILED_TEST)).toEqual(FAILED_TEST);
  });
});

describe('destination requests', () => {
  it('lets a creation leave the defaulted settings and enabled out', () => {
    const request = {
      name: 'Graylog',
      settings: { kind: AuditKind.Syslog, host: 'graylog.internal', port: 514, protocol: 'udp' },
    };
    expect(CreateDestinationRequestSchema.parse(request)).toEqual(request);
  });

  it('trims the name and refuses a blank one', () => {
    const settings = { kind: AuditKind.Webhook, url: 'https://siem.acme.com' };
    expect(CreateDestinationRequestSchema.parse({ name: ' SIEM ', settings }).name).toBe('SIEM');
    expect(CreateDestinationRequestSchema.safeParse({ name: '  ', settings }).success).toBe(false);
  });

  it('refuses a field the server does not read', () => {
    expect(
      CreateDestinationRequestSchema.safeParse({
        name: 'SIEM',
        settings: { kind: AuditKind.Webhook, url: 'https://siem.acme.com', method: 'PUT' },
      }).success
    ).toBe(false);
    expect(UpdateDestinationRequestSchema.safeParse({ kind: AuditKind.S3 }).success).toBe(false);
  });

  it('refuses secrets that mix kinds', () => {
    expect(
      UpdateDestinationRequestSchema.safeParse({
        secrets: { signing_secret: 'a', access_key_id: 'b' },
      }).success
    ).toBe(false);
  });

  it('lets a syslog update clear an optional certificate with an empty string', () => {
    expect(UpdateDestinationRequestSchema.parse({ secrets: { client_key: '' } })).toEqual({
      secrets: { client_key: '' },
    });
    expect(
      UpdateDestinationRequestSchema.safeParse({ secrets: { signing_secret: '' } }).success
    ).toBe(false);
  });

  it('sends a sparse update', () => {
    expect(UpdateDestinationRequestSchema.parse({ enabled: false })).toEqual({ enabled: false });
    expect(UpdateDestinationRequestSchema.parse({})).toEqual({});
  });

  it('accepts both syslog protocols and refuses a port out of range', () => {
    for (const protocol of SYSLOG_PROTOCOLS) {
      expect(
        UpdateDestinationRequestSchema.safeParse({
          settings: { kind: AuditKind.Syslog, host: 'h', port: 6514, protocol },
        }).success
      ).toBe(true);
    }
    expect(
      UpdateDestinationRequestSchema.safeParse({
        settings: { kind: AuditKind.Syslog, host: 'h', port: 0, protocol: SyslogProtocol.Tls },
      }).success
    ).toBe(false);
  });
});

describe('retention', () => {
  it('parses the waiting, lapsed and community states', () => {
    for (const response of [WAITING_RETENTION, LAPSED_RETENTION, COMMUNITY_RETENTION]) {
      expect(RetentionResponseSchema.parse(response)).toEqual(response);
    }
  });

  it('sends days, or null to keep events forever', () => {
    expect(UpdateRetentionRequestSchema.parse({ days: 30 })).toEqual({ days: 30 });
    expect(UpdateRetentionRequestSchema.parse({ days: null })).toEqual({ days: null });
  });

  it('refuses zero, fractions and days the server cannot hold', () => {
    for (const days of [0, -1, 1.5, RETENTION_DAYS_MAXIMUM + 1]) {
      expect(UpdateRetentionRequestSchema.safeParse({ days }).success, String(days)).toBe(false);
    }
  });
});

describe('ExportQuerySchema', () => {
  it('needs a format and both dates', () => {
    expect(
      ExportQuerySchema.safeParse({ format: 'csv', start_date: '2026-11-01', end_date: '' }).success
    ).toBe(false);
    expect(
      ExportQuerySchema.safeParse({ format: 'json', start_date: 'a', end_date: 'b' }).success
    ).toBe(false);
  });
});
