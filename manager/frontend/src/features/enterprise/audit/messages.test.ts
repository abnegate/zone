import { describe, expect, it } from 'bun:test';
import { absoluteTime } from '../../../shared/utils/time';
import { Tone } from '../status';
import {
  COMMUNITY_STANDING,
  DELIVERED_TEST,
  ENTERPRISE_STANDING,
  FAILED_TEST,
  FAILING_WEBHOOK,
  GRACE_STANDING,
  HEALTHY_S3,
  LAPSED_STANDING,
  PAUSED_SYSLOG,
  PLAIN_HTTP_WEBHOOK,
  STALLED_DESTINATIONS,
  UNVERIFIED_STANDING,
} from './fixtures';
import {
  ASK_FOR_KEY_LABEL,
  ASK_FOR_LICENSE_LABEL,
  ASK_FOR_RENEWAL_LABEL,
  DOWNLOAD_READ_ONLY,
  DOWNLOAD_UNVERIFIED,
  KEPT_FOREVER,
  LAPSED_NOTICE,
  LICENSE_LINK_LABEL,
  PASTE_KEY_LINK_LABEL,
  READ_ONLY_LAPSED,
  READ_ONLY_UNLICENSED,
  READ_ONLY_UNVERIFIED,
  RENEW_LINK_LABEL,
  RETENTION_STILL_APPLIED,
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
  countText,
  DestinationField,
  deleteMessage,
  deliveryError,
  enabledLabel,
  expiredText,
  facilityLabel,
  failureMessage,
  failureText,
  formatLabel,
  healthLabel,
  healthTone,
  heldText,
  isAuditCode,
  kindLabel,
  LicensedFeature,
  NoticeTone,
  pauseMessage,
  protocolLabel,
  readOnlyMessage,
  refusalField,
  refusalMessage,
  retentionSummary,
  retentionText,
  secretLabel,
  stalledMessage,
  standingNotice,
  testLabel,
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

  it('names Zone Enterprise as the license every audit feature needs', () => {
    expect(refusalMessage(AuditCode.LicenseRequired)).toBe(
      'Downloading the audit log or changing its destinations or retention needs a Zone Enterprise license that is active or in its grace period.'
    );
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
    [AuditCode.Rebasing]: null,
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
      `${failureMessage(DeliveryFailure.Status)} HTTP 503`
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

  it('marks a capped count as a lower bound', () => {
    expect(countText(10_000)).toBe('10,000');
    expect(countText(10_000, true)).toBe('10,000+');
    expect(backlogText(10_000, true)).toBe('10,000+ events waiting');
    expect(expiredText(10_000, true)).toBe('10,000+ events past the retention period');
    expect(heldText(10_000, true)).toBe('10,000+ events kept until delivered');
  });

  it('reports a test result', () => {
    expect(testResultMessage(DELIVERED_TEST)).toBe(TEST_DELIVERED);
    expect(testResultMessage(FAILED_TEST)).toBe(
      `The test event wasn't delivered. ${failureMessage(DeliveryFailure.Status)} HTTP 500`
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

  it('keeps a destination name with replacement patterns as typed', () => {
    expect(deleteMessage("$& $' $1")).toStartWith("Delete $& $' $1?");
  });

  it('names the destination on its test button and enabled switch', () => {
    expect(testLabel('Graylog')).toBe('Send test event to Graylog');
    expect(enabledLabel('Graylog')).toBe('Enabled for Graylog');
  });
});

const RETAINED_DAYS = 90;

describe('standing copy', () => {
  it('gives every pause its own notice', () => {
    distinct(STREAMING_PAUSES.map(pauseMessage));
  });

  it('shows the notice and link that match the license', () => {
    const upgrade = (message: string) => ({
      tone: NoticeTone.Upgrade,
      message,
      action: LICENSE_LINK_LABEL,
      request: ASK_FOR_LICENSE_LABEL,
    });
    const renew = (message: string) => ({
      tone: NoticeTone.Warning,
      message,
      action: RENEW_LINK_LABEL,
      request: ASK_FOR_RENEWAL_LABEL,
    });
    expect(standingNotice(ENTERPRISE_STANDING, null)).toBeNull();
    expect(standingNotice(GRACE_STANDING, null)).toBeNull();
    expect(standingNotice(COMMUNITY_STANDING, null)).toEqual(upgrade(UPGRADE_NOTICE));
    expect(standingNotice(LAPSED_STANDING, null)).toEqual(renew(LAPSED_NOTICE));
  });

  it('asks for the key again, not a renewal, when the stored key cannot be verified', () => {
    expect(standingNotice(UNVERIFIED_STANDING, null)).toEqual({
      tone: NoticeTone.Warning,
      message: UNVERIFIED_NOTICE,
      action: PASTE_KEY_LINK_LABEL,
      request: ASK_FOR_KEY_LABEL,
    });
    expect(PASTE_KEY_LINK_LABEL).not.toContain('Renew');
  });

  it('offers every audit feature with Zone Enterprise alone', () => {
    expect(UPGRADE_NOTICE).toBe(
      'Audit export is part of Zone Enterprise. Add a license to download the audit log, stream it to a webhook, S3 or syslog, and set a retention period.'
    );
    for (const pause of STREAMING_PAUSES) {
      expect(pauseMessage(pause)).not.toContain('Team');
    }
  });

  it('tells a lapsed admin that the backlog is kept', () => {
    expect(LAPSED_NOTICE).toContain('backlog is kept');
  });

  it('says the retention period is still applied only when one is set', () => {
    for (const standing of [LAPSED_STANDING, UNVERIFIED_STANDING]) {
      const kept = standingNotice(standing, null)?.message;
      const retained = standingNotice(standing, RETAINED_DAYS)?.message;
      expect(kept).not.toContain(RETENTION_STILL_APPLIED);
      expect(retained).toBe(`${kept} ${RETENTION_STILL_APPLIED}`);
    }
  });

  it('never claims retention is applied where it does not apply', () => {
    expect(standingNotice(COMMUNITY_STANDING, RETAINED_DAYS)?.message).not.toContain(
      RETENTION_STILL_APPLIED
    );
    expect(standingNotice({ ...LAPSED_STANDING, applies: false }, RETAINED_DAYS)?.message).toBe(
      LAPSED_NOTICE
    );
  });
});

describe('readOnlyMessage', () => {
  it('says nothing when the license covers the feature', () => {
    for (const feature of Object.values(LicensedFeature)) {
      expect(readOnlyMessage(ENTERPRISE_STANDING, feature)).toBeNull();
    }
  });

  it('says what would let each license change streaming and retention', () => {
    for (const feature of [LicensedFeature.Streaming, LicensedFeature.Retention]) {
      expect(readOnlyMessage(COMMUNITY_STANDING, feature)).toBe(READ_ONLY_UNLICENSED);
      expect(readOnlyMessage(LAPSED_STANDING, feature)).toBe(READ_ONLY_LAPSED);
      expect(readOnlyMessage(UNVERIFIED_STANDING, feature)).toBe(READ_ONLY_UNVERIFIED);
    }
  });

  it('gives downloads their own wording', () => {
    for (const standing of [COMMUNITY_STANDING, LAPSED_STANDING]) {
      expect(readOnlyMessage(standing, LicensedFeature.Export)).toBe(DOWNLOAD_READ_ONLY);
    }
    expect(DOWNLOAD_READ_ONLY).toBe(
      'Downloads need a Zone Enterprise license that is active or in its grace period.'
    );
  });

  it('asks for the key again to download when the stored key cannot be verified', () => {
    expect(readOnlyMessage(UNVERIFIED_STANDING, LicensedFeature.Export)).toBe(DOWNLOAD_UNVERIFIED);
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

  it('summarizes a period the license applies like any other', () => {
    expect(retentionSummary(90, true)).toBe(retentionText(90));
    expect(retentionSummary(null, true)).toBe(KEPT_FOREVER);
    expect(retentionSummary(null, false)).toBe(KEPT_FOREVER);
  });

  it('says a saved period resumes once a license is installed', () => {
    const summary = retentionSummary(90, false);
    expect(summary).toContain('90 days is saved');
    expect(summary).toContain('once a Zone license is installed');
    expect(summary).not.toContain('are deleted once');
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
