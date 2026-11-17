import { absoluteTime } from '../../../shared/utils/time';
import { LicenseStatus } from '../schemas';
import { Tone } from '../status';
import {
  BACKLOG_WAITING,
  CAPPED_COUNT,
  DAY_PLURAL,
  DAY_SINGULAR,
  DELETE_CONFIRMATION,
  DELETED_COUNT,
  DOWNLOAD_READ_ONLY,
  DOWNLOAD_UNVERIFIED,
  EDITION_NOTICE,
  ENABLED_DESTINATION_LABEL,
  ENTERPRISE_NOTICE,
  EVENT_PLURAL,
  EVENT_SINGULAR,
  EXPIRED_COUNT,
  HELD_COUNT,
  KEPT_FOREVER,
  LAPSED_NOTICE,
  LAPSED_TEAM_NOTICE,
  LICENSE_LINK_LABEL,
  PASTE_KEY_LINK_LABEL,
  READ_ONLY_EDITION,
  READ_ONLY_LAPSED,
  READ_ONLY_UNLICENSED,
  READ_ONLY_UNVERIFIED,
  RENEW_LINK_LABEL,
  RETENTION_PERIOD,
  RETENTION_PERIOD_SAVED,
  RETENTION_STILL_APPLIED,
  STALLED_NOTICE,
  TEST_DELIVERED,
  TEST_DESTINATION_LABEL,
  TEST_NOT_DELIVERED,
  UNENCRYPTED_ENDPOINT_WARNING,
  UNENCRYPTED_HTTP_WARNING,
  UNENCRYPTED_UDP_WARNING,
  UNVERIFIED_NOTICE,
  UP_TO_DATE,
  UPGRADE_NOTICE,
} from './labels';
import {
  AuditCode,
  AuditKind,
  DeliveryFailure,
  DeliveryHealth,
  ExportFormat,
  RetentionWait,
  S3Addressing,
  SecretName,
  StreamingPause,
  SyslogProtocol,
} from './schemas';
import type {
  AuditCode as AuditCodeValue,
  AuditKind as AuditKindValue,
  DeliveryFailure as DeliveryFailureValue,
  DeliveryHealth as DeliveryHealthValue,
  DeliveryResponse,
  ExportFormat as ExportFormatValue,
  RetentionWait as RetentionWaitValue,
  S3Addressing as S3AddressingValue,
  SecretName as SecretNameValue,
  SettingsRequest,
  Standing,
  StreamingPause as StreamingPauseValue,
  SyslogProtocol as SyslogProtocolValue,
  TestResponse,
} from './types';

export const DestinationField = {
  Name: 'name',
  Kind: 'kind',
  Url: 'url',
  Bucket: 'bucket',
  Region: 'region',
  Endpoint: 'endpoint',
  Prefix: 'prefix',
  Host: 'host',
  Port: 'port',
  Facility: 'facility',
  Hostname: 'hostname',
  Secrets: 'secrets',
} as const;

export type DestinationField = (typeof DestinationField)[keyof typeof DestinationField];

export const LicensedFeature = {
  Export: 'export',
  Streaming: 'streaming',
  Retention: 'retention',
} as const;

export type LicensedFeature = (typeof LicensedFeature)[keyof typeof LicensedFeature];

export const NoticeTone = {
  Upgrade: 'upgrade',
  Warning: 'warning',
} as const;

export type NoticeTone = (typeof NoticeTone)[keyof typeof NoticeTone];

export interface Notice {
  tone: NoticeTone;
  message: string;
  action: string;
}

const REFUSALS: Readonly<Record<AuditCodeValue, string>> = {
  [AuditCode.LicenseRequired]:
    'This needs a Zone license that covers it: downloads need Team or Enterprise, and streaming and retention need Enterprise, active or in its grace period.',
  [AuditCode.BodyInvalid]: "Zone couldn't read this change. Reload the page and try again.",
  [AuditCode.DestinationNotFound]:
    'This destination no longer exists. It may have been deleted somewhere else.',
  [AuditCode.DestinationDuplicate]: 'A destination with this name already exists.',
  [AuditCode.NameInvalid]: 'Enter a name without line breaks or other control characters.',
  [AuditCode.KindInvalid]:
    "Choose a webhook, S3 or syslog destination. A destination's type can't change after it is added.",
  [AuditCode.UrlInvalid]:
    'Enter an http or https URL with a host and no user name, query or fragment. Cloud metadata addresses and hosts outside ZONE_ENDPOINT_HOSTS are refused.',
  [AuditCode.BucketInvalid]:
    'Enter a bucket name of 3 to 63 lowercase letters, digits, dots or dashes, starting and ending with a letter or digit.',
  [AuditCode.RegionInvalid]:
    'Enter a region like us-east-1, in lowercase letters, digits and dashes.',
  [AuditCode.PrefixInvalid]: 'Enter a key prefix like zone/audit/, without control characters.',
  [AuditCode.HostInvalid]:
    'Enter a hostname or IP address without a scheme or port. Cloud metadata addresses and hosts outside ZONE_ENDPOINT_HOSTS are refused.',
  [AuditCode.PortInvalid]: 'Enter a port from 1 to 65535.',
  [AuditCode.FacilityInvalid]: 'Choose a syslog facility from 0 to 23.',
  [AuditCode.HostnameInvalid]:
    'Enter a hostname of up to 255 printable ASCII characters, without spaces.',
  [AuditCode.SecretInvalid]:
    'Enter every secret this destination needs: a signing secret for a webhook, or an access key ID and secret access key for S3.',
  [AuditCode.CertificateInvalid]:
    'Enter PEM certificates, with the client certificate and key together. Certificates only apply to syslog over TLS.',
  [AuditCode.RetentionInvalid]: 'Enter a whole number of days, at least 1, or keep events forever.',
  [AuditCode.ExportRangeInvalid]:
    'Choose a start date and an end date, with the start on or before the end.',
  [AuditCode.ExportFormatInvalid]: 'Choose NDJSON or CSV.',
};

const REFUSAL_FIELDS: Readonly<Partial<Record<AuditCodeValue, DestinationField>>> = {
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
};

const FAILURES: Readonly<Record<DeliveryFailureValue, string>> = {
  [DeliveryFailure.Connect]: "Zone couldn't connect to the destination.",
  [DeliveryFailure.Timeout]: 'The destination took too long to answer.',
  [DeliveryFailure.Tls]:
    "The TLS handshake failed. Check the destination's certificate and the CA certificate saved here.",
  [DeliveryFailure.Status]: 'The destination answered with an error.',
  [DeliveryFailure.Target]:
    'The destination resolved to an address Zone never sends to, such as a cloud metadata address.',
  [DeliveryFailure.Configuration]:
    "The destination's settings can't be used, for example because its host is no longer allowed by ZONE_ENDPOINT_HOSTS.",
  [DeliveryFailure.Unsealable]:
    "Zone couldn't decrypt this destination's secrets, which happens when the encryption key changed. Enter the secrets again.",
};

const PAUSES: Readonly<Record<StreamingPauseValue, string>> = {
  [StreamingPause.Unlicensed]: UPGRADE_NOTICE,
  [StreamingPause.Edition]: EDITION_NOTICE,
  [StreamingPause.Lapsed]: LAPSED_NOTICE,
  [StreamingPause.Unverified]: UNVERIFIED_NOTICE,
};

const READ_ONLY: Readonly<Record<StreamingPauseValue, string>> = {
  [StreamingPause.Unlicensed]: READ_ONLY_UNLICENSED,
  [StreamingPause.Edition]: READ_ONLY_EDITION,
  [StreamingPause.Lapsed]: READ_ONLY_LAPSED,
  [StreamingPause.Unverified]: READ_ONLY_UNVERIFIED,
};

const UPGRADES: ReadonlySet<StreamingPauseValue> = new Set([
  StreamingPause.Unlicensed,
  StreamingPause.Edition,
]);

const WAITS: Readonly<Record<RetentionWaitValue, string>> = {
  [RetentionWait.Backlog]: 'Still delivering events from before the cutoff.',
  [RetentionWait.Failing]: 'Delivery is failing, so events are kept until it recovers.',
  [RetentionWait.Paused]: 'Streaming is paused, so events are kept until it resumes.',
};

const HEALTH_LABELS: Readonly<Record<DeliveryHealthValue, string>> = {
  [DeliveryHealth.Healthy]: 'Healthy',
  [DeliveryHealth.Failing]: 'Failing',
  [DeliveryHealth.Paused]: 'Paused',
  [DeliveryHealth.Disabled]: 'Disabled',
};

const HEALTH_TONES: Readonly<Record<DeliveryHealthValue, Tone>> = {
  [DeliveryHealth.Healthy]: Tone.Success,
  [DeliveryHealth.Failing]: Tone.Error,
  [DeliveryHealth.Paused]: Tone.Warning,
  [DeliveryHealth.Disabled]: Tone.Neutral,
};

const KIND_LABELS: Readonly<Record<AuditKindValue, string>> = {
  [AuditKind.Webhook]: 'Webhook',
  [AuditKind.S3]: 'S3',
  [AuditKind.Syslog]: 'Syslog',
};

const PROTOCOL_LABELS: Readonly<Record<SyslogProtocolValue, string>> = {
  [SyslogProtocol.Tls]: 'TCP with TLS',
  [SyslogProtocol.Udp]: 'UDP',
};

const ADDRESSING_LABELS: Readonly<Record<S3AddressingValue, string>> = {
  [S3Addressing.VirtualHost]: 'Virtual host (bucket.endpoint)',
  [S3Addressing.Path]: 'Path (endpoint/bucket)',
};

const FORMAT_LABELS: Readonly<Record<ExportFormatValue, string>> = {
  [ExportFormat.Ndjson]: 'NDJSON',
  [ExportFormat.Csv]: 'CSV',
};

const SECRET_LABELS: Readonly<Record<SecretNameValue, string>> = {
  [SecretName.SigningSecret]: 'Signing secret',
  [SecretName.AccessKeyId]: 'Access key ID',
  [SecretName.SecretAccessKey]: 'Secret access key',
  [SecretName.CaCertificate]: 'CA certificate',
  [SecretName.ClientCertificate]: 'Client certificate',
  [SecretName.ClientKey]: 'Client key',
};

const FACILITY_NAMES = [
  'kernel',
  'user',
  'mail',
  'daemon',
  'security',
  'syslog',
  'printer',
  'news',
  'uucp',
  'clock',
  'authorization',
  'ftp',
  'ntp',
  'log audit',
  'log alert',
  'clock daemon',
  'local0',
  'local1',
  'local2',
  'local3',
  'local4',
  'local5',
  'local6',
  'local7',
] as const;

const PLAIN_HTTP = /^\s*http:/i;

const SLOT = '{}';

const ADMIN_CODES: ReadonlySet<string> = new Set(Object.keys(REFUSALS));

function fill(template: string, value: string): string {
  return template.replace(SLOT, () => value);
}

function counted(count: number, singular: string, plural: string, capped = false): string {
  return `${countText(count, capped)} ${count === 1 && !capped ? singular : plural}`;
}

function events(count: number, capped = false): string {
  return counted(count, EVENT_SINGULAR, EVENT_PLURAL, capped);
}

function days(count: number): string {
  return counted(count, DAY_SINGULAR, DAY_PLURAL);
}

export function isAuditCode(code: string): code is AuditCodeValue {
  return ADMIN_CODES.has(code);
}

export function refusalMessage(code: AuditCodeValue): string {
  return REFUSALS[code];
}

export function refusalField(
  code: AuditCodeValue,
  kind: AuditKindValue | null = null
): DestinationField | null {
  if (code === AuditCode.UrlInvalid && kind === AuditKind.S3) {
    return DestinationField.Endpoint;
  }
  return REFUSAL_FIELDS[code] ?? null;
}

export function failureMessage(code: DeliveryFailureValue): string {
  return FAILURES[code];
}

export function failureText(code: DeliveryFailureValue | null, detail: string | null): string {
  const reason = detail?.trim() ?? '';
  if (code === null) {
    return reason;
  }
  const message = failureMessage(code);
  return reason === '' || reason === message ? message : `${message} ${reason}`;
}

export function deliveryError(delivery: DeliveryResponse): string | null {
  if (delivery.last_error_code === null && delivery.last_error === null) {
    return null;
  }
  return failureText(delivery.last_error_code, delivery.last_error);
}

export function testResultMessage(result: TestResponse): string {
  if (result.delivered) {
    return TEST_DELIVERED;
  }
  return fill(TEST_NOT_DELIVERED, failureText(result.error_code, result.error)).trim();
}

export function testLabel(name: string): string {
  return fill(TEST_DESTINATION_LABEL, name);
}

export function enabledLabel(name: string): string {
  return fill(ENABLED_DESTINATION_LABEL, name);
}

export function pauseMessage(pause: StreamingPauseValue): string {
  return PAUSES[pause];
}

function pausedMessage(standing: Standing, pause: StreamingPauseValue): string {
  if (pause !== StreamingPause.Edition) {
    return pauseMessage(pause);
  }
  if (standing.status === LicenseStatus.Lapsed) {
    return LAPSED_TEAM_NOTICE;
  }
  return standing.export ? EDITION_NOTICE : ENTERPRISE_NOTICE;
}

function noticeAction(upgrade: boolean, pause: StreamingPauseValue): string {
  if (upgrade) {
    return LICENSE_LINK_LABEL;
  }
  return pause === StreamingPause.Unverified ? PASTE_KEY_LINK_LABEL : RENEW_LINK_LABEL;
}

function noticeMessage(
  standing: Standing,
  pause: StreamingPauseValue,
  upgrade: boolean,
  days: number | null
): string {
  const message = pausedMessage(standing, pause);
  const retained = !upgrade && standing.applies && days !== null;
  return retained ? `${message} ${RETENTION_STILL_APPLIED}` : message;
}

export function standingNotice(standing: Standing, days: number | null): Notice | null {
  const { pause } = standing;
  if (pause === null) {
    return null;
  }
  const upgrade = UPGRADES.has(pause) && standing.status !== LicenseStatus.Lapsed;
  return {
    tone: upgrade ? NoticeTone.Upgrade : NoticeTone.Warning,
    message: noticeMessage(standing, pause, upgrade, days),
    action: noticeAction(upgrade, pause),
  };
}

export function readOnlyMessage(standing: Standing, feature: LicensedFeature): string | null {
  if (standing[feature]) {
    return null;
  }
  if (feature === LicensedFeature.Export) {
    return standing.pause === StreamingPause.Unverified ? DOWNLOAD_UNVERIFIED : DOWNLOAD_READ_ONLY;
  }
  return READ_ONLY[standing.pause ?? StreamingPause.Lapsed];
}

export function waitMessage(wait: RetentionWaitValue): string {
  return WAITS[wait];
}

export function healthLabel(health: DeliveryHealthValue): string {
  return HEALTH_LABELS[health];
}

export function healthTone(health: DeliveryHealthValue): Tone {
  return HEALTH_TONES[health];
}

export function kindLabel(kind: AuditKindValue): string {
  return KIND_LABELS[kind];
}

export function protocolLabel(protocol: SyslogProtocolValue): string {
  return PROTOCOL_LABELS[protocol];
}

export function addressingLabel(addressing: S3AddressingValue): string {
  return ADDRESSING_LABELS[addressing];
}

export function formatLabel(format: ExportFormatValue): string {
  return FORMAT_LABELS[format];
}

export function secretLabel(name: SecretNameValue): string {
  return SECRET_LABELS[name];
}

export function facilityLabel(facility: number): string {
  const name = FACILITY_NAMES[facility];
  return name === undefined ? String(facility) : `${facility} (${name})`;
}

export function stalledMessage(stalledSince: string): string {
  return fill(STALLED_NOTICE, absoluteTime(stalledSince));
}

export function unencryptedWarning(settings: SettingsRequest): string | null {
  switch (settings.kind) {
    case AuditKind.Webhook:
      return PLAIN_HTTP.test(settings.url) ? UNENCRYPTED_HTTP_WARNING : null;
    case AuditKind.S3:
      return PLAIN_HTTP.test(settings.endpoint ?? '') ? UNENCRYPTED_ENDPOINT_WARNING : null;
    case AuditKind.Syslog:
      return settings.protocol === SyslogProtocol.Udp ? UNENCRYPTED_UDP_WARNING : null;
  }
}

export function countText(count: number, capped = false): string {
  const text = count.toLocaleString();
  return capped ? fill(CAPPED_COUNT, text) : text;
}

export function backlogText(backlog: number, capped = false): string {
  return backlog === 0 ? UP_TO_DATE : fill(BACKLOG_WAITING, events(backlog, capped));
}

export function retentionText(period: number | null): string {
  return period === null ? KEPT_FOREVER : fill(RETENTION_PERIOD, days(period));
}

export function retentionSummary(period: number | null, applies: boolean): string {
  return applies || period === null
    ? retentionText(period)
    : fill(RETENTION_PERIOD_SAVED, days(period));
}

export function expiredText(expired: number, capped = false): string {
  return fill(EXPIRED_COUNT, events(expired, capped));
}

export function heldText(held: number, capped = false): string {
  return fill(HELD_COUNT, events(held, capped));
}

export function deletedText(deleted: number): string {
  return fill(DELETED_COUNT, events(deleted));
}

export function deleteMessage(name: string): string {
  return fill(DELETE_CONFIRMATION, name);
}
