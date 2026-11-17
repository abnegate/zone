import { absoluteTime } from '../../../shared/utils/time';
import { Tone } from '../status';
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

const ADMIN_CODES: ReadonlySet<string> = new Set(Object.keys(REFUSALS));

function plural(count: number, noun: string): string {
  return `${count.toLocaleString()} ${noun}${count === 1 ? '' : 's'}`;
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
  return `The test event wasn't delivered. ${failureText(result.error_code, result.error)}`.trim();
}

export function pauseMessage(pause: StreamingPauseValue): string {
  return PAUSES[pause];
}

export function standingNotice(standing: Standing): string | null {
  return standing.pause === null ? null : pauseMessage(standing.pause);
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
  return `Delivery is waiting behind a database transaction that has been open since ${absoluteTime(stalledSince)}. Events written after it are sent once it finishes; nothing is lost.`;
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

export function backlogText(backlog: number): string {
  return backlog === 0 ? UP_TO_DATE : `${plural(backlog, 'event')} waiting`;
}

export function retentionText(days: number | null): string {
  return days === null
    ? KEPT_FOREVER
    : `Events older than ${plural(days, 'day')} are deleted once every enabled destination has delivered them.`;
}

export function expiredText(expired: number): string {
  return `${plural(expired, 'event')} past the retention period`;
}

export function heldText(held: number): string {
  return `${plural(held, 'event')} kept until delivered`;
}

export function deletedText(deleted: number): string {
  return `${plural(deleted, 'event')} deleted`;
}

export function deleteMessage(name: string): string {
  return `Delete ${name}? Zone stops sending audit events to it and forgets its secrets. Events it hasn't received are no longer held back from retention.`;
}
