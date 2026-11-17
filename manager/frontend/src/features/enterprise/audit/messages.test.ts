import { describe, expect, it } from 'bun:test';
import { absoluteTime } from '../../../shared/utils/time';
import { Tone } from '../status';
import {
  COMMUNITY_STANDING,
  DELIVERED_TEST,
  ENTERPRISE_STANDING,
  FAILED_TEST,
  FAILING_WEBHOOK,
  HEALTHY_S3,
  LAPSED_STANDING,
  PAUSED_SYSLOG,
  PLAIN_HTTP_WEBHOOK,
  STALLED_DESTINATIONS,
  TEAM_STANDING,
  UNVERIFIED_STANDING,
} from './fixtures';
import {
  EDITION_NOTICE,
  KEPT_FOREVER,
  LAPSED_NOTICE,
  TEST_DELIVERED,
  UNENCRYPTED_ENDPOINT_WARNING,
  UNENCRYPTED_HTTP_WARNING,
  UNENCRYPTED_UDP_WARNING,
  UNVERIFIED_NOTICE,
  UP_TO_DATE,
  UPGRADE_NOTICE,
} from './labels';
import {
  addressingLabel,
  backlogText,
  DestinationField,
  deleteMessage,
  deliveryError,
  facilityLabel,
  failureMessage,
  failureText,
  formatLabel,
  healthLabel,
  healthTone,
  heldText,
  isAuditCode,
  kindLabel,
  pauseMessage,
  protocolLabel,
  refusalField,
  refusalMessage,
  retentionText,
  secretLabel,
  stalledMessage,
  standingNotice,
  testResultMessage,
  unencryptedWarning,
  waitMessage,
} from './messages';
import {
  AUDIT_ADMIN_CODES,
  AUDIT_KINDS,
  AuditCode,
  AuditKind,
  DELIVERY_FAILURES,
  DELIVERY_HEALTHS,
  DeliveryFailure,
  DeliveryHealth,
  EXPORT_FORMATS,
  FACILITY_MAXIMUM,
  RETENTION_WAITS,
  S3_ADDRESSINGS,
  SECRET_NAMES,
  STREAMING_PAUSES,
  SYSLOG_PROTOCOLS,
  SyslogProtocol,
} from './schemas';

function distinct(values: readonly string[]): void {
  expect(new Set(values).size).toBe(values.length);
  for (const value of values) {
    expect(value.trim().length).toBeGreaterThan(0);
  }
}

describe('refusalMessage', () => {
  for (const code of AUDIT_ADMIN_CODES) {
    it(`explains ${code} in a sentence`, () => {
      expect(refusalMessage(code)).toEndWith('.');
    });
  }

  it('gives every code its own message', () => {
    distinct(AUDIT_ADMIN_CODES.map(refusalMessage));
  });

  it('names Team for downloads and Enterprise for streaming when a license is required', () => {
    expect(refusalMessage(AuditCode.LicenseRequired)).toContain('Team or Enterprise');
    expect(refusalMessage(AuditCode.LicenseRequired)).toContain('need Enterprise');
  });

  it('recognises only audit codes', () => {
    for (const code of AUDIT_ADMIN_CODES) {
      expect(isAuditCode(code)).toBe(true);
    }
    expect(isAuditCode('policy_body_invalid')).toBe(false);
  });
});

describe('refusalField', () => {
  const expected: Record<string, string | null> = {
    [AuditCode.LicenseRequired]: null,
    [AuditCode.BodyInvalid]: null,
    [AuditCode.DestinationNotFound]: null,
    [AuditCode.DestinationDuplicate]: DestinationField.Name,
    [AuditCode.NameInvalid]: DestinationField.Name,
    [AuditCode.KindInvalid]: DestinationField.Kind,
    [AuditCode.UrlInvalid]: DestinationField.Url,
    [AuditCode.BucketInvalid]: DestinationField.Bucket,
    [AuditCode.RegionInvalid]: DestinationField.Region,
    [AuditCode.PrefixInvalid]: DestinationField.Prefix,
    [AuditCode.HostInvalid]: DestinationField.Host,
    [AuditCode.PortInvalid]: DestinationField.Port,
    [AuditCode.FacilityInvalid]: DestinationField.Facility,
    [AuditCode.HostnameInvalid]: DestinationField.Hostname,
    [AuditCode.SecretInvalid]: DestinationField.Secrets,
    [AuditCode.CertificateInvalid]: DestinationField.Secrets,
    [AuditCode.RetentionInvalid]: null,
    [AuditCode.ExportRangeInvalid]: null,
    [AuditCode.ExportFormatInvalid]: null,
  };

  for (const code of AUDIT_ADMIN_CODES) {
    it(`puts ${code} on ${expected[code] ?? 'the whole form'}`, () => {
      expect(refusalField(code)).toBe(expected[code]);
    });
  }

  it('puts an invalid URL on the endpoint of an S3 destination', () => {
    expect(refusalField(AuditCode.UrlInvalid, AuditKind.S3)).toBe(DestinationField.Endpoint);
    expect(refusalField(AuditCode.UrlInvalid, AuditKind.Webhook)).toBe(DestinationField.Url);
  });
});

describe('delivery copy', () => {
  it('explains every failure without repeating itself', () => {
    distinct(DELIVERY_FAILURES.map(failureMessage));
  });

  it('adds the server detail to a failure once', () => {
    expect(failureText(DeliveryFailure.Status, 'Answered 503.')).toBe(
      `${failureMessage(DeliveryFailure.Status)} Answered 503.`
    );
    expect(failureText(DeliveryFailure.Status, '  ')).toBe(failureMessage(DeliveryFailure.Status));
    expect(failureText(null, 'Answered 503.')).toBe('Answered 503.');
  });

  it('describes the last error of a failing destination, and nothing for a healthy one', () => {
    expect(deliveryError(FAILING_WEBHOOK.delivery)).toBe(
      `${failureMessage(DeliveryFailure.Status)} The receiver answered 503 Service Unavailable.`
    );
    expect(deliveryError(HEALTHY_S3.delivery)).toBeNull();
  });

  it('labels and tones every health', () => {
    distinct(DELIVERY_HEALTHS.map(healthLabel));
    expect(DELIVERY_HEALTHS.map(healthTone)).toEqual([
      Tone.Success,
      Tone.Error,
      Tone.Warning,
      Tone.Neutral,
    ]);
    expect(healthLabel(DeliveryHealth.Paused)).toBe('Paused');
  });

  it('counts the backlog', () => {
    expect(backlogText(0)).toBe(UP_TO_DATE);
    expect(backlogText(1)).toBe('1 event waiting');
    expect(backlogText(1_204)).toBe('1,204 events waiting');
  });

  it('reports a test result', () => {
    expect(testResultMessage(DELIVERED_TEST)).toBe(TEST_DELIVERED);
    expect(testResultMessage(FAILED_TEST)).toBe(
      `The test event wasn't delivered. ${failureMessage(DeliveryFailure.Status)} The receiver answered 500 Internal Server Error.`
    );
  });

  it('dates a stalled delivery', () => {
    const since = STALLED_DESTINATIONS.stalled_since ?? '';
    expect(stalledMessage(since)).toContain(absoluteTime(since));
    expect(stalledMessage(since)).toContain('nothing is lost');
  });

  it('names the destination being deleted', () => {
    expect(deleteMessage('Graylog')).toStartWith('Delete Graylog?');
  });
});

describe('standing copy', () => {
  it('gives every pause its own notice', () => {
    distinct(STREAMING_PAUSES.map(pauseMessage));
  });

  it('shows the notice that matches the license', () => {
    expect(standingNotice(ENTERPRISE_STANDING)).toBeNull();
    expect(standingNotice(COMMUNITY_STANDING)).toBe(UPGRADE_NOTICE);
    expect(standingNotice(TEAM_STANDING)).toBe(EDITION_NOTICE);
    expect(standingNotice(LAPSED_STANDING)).toBe(LAPSED_NOTICE);
    expect(standingNotice(UNVERIFIED_STANDING)).toBe(UNVERIFIED_NOTICE);
  });

  it('tells a lapsed admin that the backlog is kept and retention still applies', () => {
    expect(LAPSED_NOTICE).toContain('backlog is kept');
    expect(LAPSED_NOTICE).toContain('still applied');
  });
});

describe('retention copy', () => {
  it('explains every wait', () => {
    distinct(RETENTION_WAITS.map(waitMessage));
  });

  it('describes the period', () => {
    expect(retentionText(null)).toBe(KEPT_FOREVER);
    expect(retentionText(1)).toStartWith('Events older than 1 day are deleted');
    expect(retentionText(90)).toStartWith('Events older than 90 days are deleted');
  });

  it('counts the held events', () => {
    expect(heldText(342)).toBe('342 events kept until delivered');
  });
});

describe('unencryptedWarning', () => {
  it('warns about a plain http webhook in any case', () => {
    expect(unencryptedWarning(PLAIN_HTTP_WEBHOOK.settings)).toBe(UNENCRYPTED_HTTP_WARNING);
    expect(unencryptedWarning({ kind: AuditKind.Webhook, url: ' HTTP://10.0.0.5/hook' })).toBe(
      UNENCRYPTED_HTTP_WARNING
    );
  });

  it('stays quiet for https', () => {
    expect(unencryptedWarning(FAILING_WEBHOOK.settings)).toBeNull();
    expect(unencryptedWarning(HEALTHY_S3.settings)).toBeNull();
    expect(unencryptedWarning(PAUSED_SYSLOG.settings)).toBeNull();
  });

  it('warns about a plain http S3 endpoint', () => {
    expect(unencryptedWarning({ ...HEALTHY_S3.settings, endpoint: 'http://minio:9000' })).toBe(
      UNENCRYPTED_ENDPOINT_WARNING
    );
  });

  it('warns about UDP syslog', () => {
    expect(
      unencryptedWarning({
        kind: AuditKind.Syslog,
        host: '10.0.0.5',
        port: 514,
        protocol: SyslogProtocol.Udp,
      })
    ).toBe(UNENCRYPTED_UDP_WARNING);
  });
});

describe('vocabulary labels', () => {
  it('labels every kind, protocol, addressing, format and secret distinctly', () => {
    distinct(AUDIT_KINDS.map(kindLabel));
    distinct(SYSLOG_PROTOCOLS.map(protocolLabel));
    distinct(S3_ADDRESSINGS.map(addressingLabel));
    distinct(EXPORT_FORMATS.map(formatLabel));
    distinct(SECRET_NAMES.map(secretLabel));
  });

  it('names every syslog facility', () => {
    const labels = Array.from({ length: FACILITY_MAXIMUM + 1 }, (_, facility) =>
      facilityLabel(facility)
    );
    distinct(labels);
    expect(facilityLabel(13)).toBe('13 (log audit)');
    expect(facilityLabel(24)).toBe('24');
  });
});
