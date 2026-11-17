import {
  BUCKET_REQUIRED,
  CLIENT_PAIR_REQUIRED,
  HOST_REQUIRED,
  NAME_REQUIRED,
  REGION_REQUIRED,
  SECRET_REQUIRED,
  URL_REQUIRED,
} from './labels';
import { DestinationField, refusalMessage } from './messages';
import {
  AuditCode,
  AuditKind,
  DEFAULT_ADDRESSING,
  DEFAULT_FACILITY,
  DEFAULT_SYSLOG_HOSTNAME,
  KIND_SECRETS,
  PORT_MAXIMUM,
  PORT_MINIMUM,
  REQUIRED_SECRETS,
  SECRET_NAMES,
  SecretName,
  SYSLOG_PORTS,
  SyslogProtocol,
} from './schemas';
import type {
  AuditKind as AuditKindValue,
  CreateDestinationRequest,
  DestinationResponse,
  S3Addressing,
  SecretName as SecretNameValue,
  SecretsRequest,
  SettingsRequest,
  SyslogProtocol as SyslogProtocolValue,
  UpdateDestinationRequest,
} from './types';

export interface DestinationDraft {
  name: string;
  kind: AuditKindValue;
  enabled: boolean;
  url: string;
  bucket: string;
  region: string;
  endpoint: string;
  prefix: string;
  addressing: S3Addressing;
  host: string;
  port: string;
  protocol: SyslogProtocolValue;
  facility: number;
  hostname: string;
  secrets: Readonly<Record<SecretNameValue, string>>;
  cleared: readonly SecretNameValue[];
}

export type DraftErrors = Partial<Record<DestinationField | SecretNameValue, string>>;

const CERTIFICATES: readonly SecretNameValue[] = KIND_SECRETS[AuditKind.Syslog];

const SECRET_BYTES = 32;

const INTEGER = /^\d+$/;

const NO_SECRETS = Object.fromEntries(SECRET_NAMES.map((name) => [name, ''])) as Record<
  SecretNameValue,
  string
>;

export function emptyDraft(kind: AuditKindValue = AuditKind.Webhook): DestinationDraft {
  return {
    name: '',
    kind,
    enabled: true,
    url: '',
    bucket: '',
    region: '',
    endpoint: '',
    prefix: '',
    addressing: DEFAULT_ADDRESSING,
    host: '',
    port: String(SYSLOG_PORTS[SyslogProtocol.Tls]),
    protocol: SyslogProtocol.Tls,
    facility: DEFAULT_FACILITY,
    hostname: DEFAULT_SYSLOG_HOSTNAME,
    secrets: NO_SECRETS,
    cleared: [],
  };
}

export function draftOf(destination: DestinationResponse): DestinationDraft {
  const draft: DestinationDraft = {
    ...emptyDraft(destination.kind),
    name: destination.name,
    enabled: destination.enabled,
  };
  const { settings } = destination;
  switch (settings.kind) {
    case AuditKind.Webhook:
      return { ...draft, url: settings.url };
    case AuditKind.S3:
      return {
        ...draft,
        bucket: settings.bucket,
        region: settings.region,
        endpoint: settings.endpoint ?? '',
        prefix: settings.prefix,
        addressing: settings.addressing,
      };
    case AuditKind.Syslog:
      return {
        ...draft,
        host: settings.host,
        port: String(settings.port),
        protocol: settings.protocol,
        facility: settings.facility,
        hostname: settings.hostname,
      };
  }
}

export function withProtocol(
  draft: DestinationDraft,
  protocol: SyslogProtocolValue
): DestinationDraft {
  const defaulted = draft.port.trim() === String(SYSLOG_PORTS[draft.protocol]);
  return { ...draft, protocol, port: defaulted ? String(SYSLOG_PORTS[protocol]) : draft.port };
}

export function certificatesAllowed(draft: DestinationDraft): boolean {
  return draft.kind === AuditKind.Syslog && draft.protocol === SyslogProtocol.Tls;
}

export function draftSecrets(draft: DestinationDraft): readonly SecretNameValue[] {
  if (draft.kind === AuditKind.Syslog && !certificatesAllowed(draft)) {
    return [];
  }
  return KIND_SECRETS[draft.kind];
}

export function clearable(name: SecretNameValue): boolean {
  return CERTIFICATES.includes(name);
}

export function settingsOf(draft: DestinationDraft): SettingsRequest {
  switch (draft.kind) {
    case AuditKind.Webhook:
      return { kind: AuditKind.Webhook, url: draft.url.trim() };
    case AuditKind.S3: {
      const endpoint = draft.endpoint.trim();
      return {
        kind: AuditKind.S3,
        bucket: draft.bucket.trim(),
        region: draft.region.trim(),
        endpoint: endpoint === '' ? null : endpoint,
        prefix: draft.prefix.trim(),
        addressing: draft.addressing,
      };
    }
    case AuditKind.Syslog: {
      const hostname = draft.hostname.trim();
      return {
        kind: AuditKind.Syslog,
        host: draft.host.trim(),
        port: Number(draft.port.trim()),
        protocol: draft.protocol,
        facility: draft.facility,
        hostname: hostname === '' ? DEFAULT_SYSLOG_HOSTNAME : hostname,
      };
    }
  }
}

function typed(draft: DestinationDraft): Partial<Record<SecretNameValue, string>> {
  return Object.fromEntries(
    draftSecrets(draft)
      .map((name) => [name, draft.secrets[name].trim()] as const)
      .filter(([, value]) => value !== '')
  );
}

function removed(
  draft: DestinationDraft,
  saved: readonly SecretNameValue[]
): Partial<Record<SecretNameValue, string>> {
  if (draft.kind !== AuditKind.Syslog) {
    return {};
  }
  const dropped = certificatesAllowed(draft) ? draft.cleared : CERTIFICATES;
  return Object.fromEntries(
    dropped.filter((name) => saved.includes(name)).map((name) => [name, ''] as const)
  );
}

function secretsRequest(
  secrets: Partial<Record<SecretNameValue, string>>
): { secrets: SecretsRequest } | Record<string, never> {
  return Object.keys(secrets).length > 0 ? { secrets: secrets as SecretsRequest } : {};
}

export function createRequest(draft: DestinationDraft): CreateDestinationRequest {
  return {
    name: draft.name.trim(),
    enabled: draft.enabled,
    settings: settingsOf(draft),
    ...secretsRequest(typed(draft)),
  };
}

export function updateRequest(
  destination: DestinationResponse,
  draft: DestinationDraft
): UpdateDestinationRequest {
  const name = draft.name.trim();
  const settings = settingsOf(draft);
  const settingsChanged =
    JSON.stringify(settings) !== JSON.stringify(settingsOf(draftOf(destination)));
  return {
    ...(name !== destination.name ? { name } : {}),
    ...(draft.enabled !== destination.enabled ? { enabled: draft.enabled } : {}),
    ...(settingsChanged ? { settings } : {}),
    ...secretsRequest({ ...removed(draft, destination.secrets), ...typed(draft) }),
  };
}

export function changed(request: UpdateDestinationRequest): boolean {
  return Object.keys(request).length > 0;
}

function kept(
  draft: DestinationDraft,
  saved: readonly SecretNameValue[],
  name: SecretNameValue
): boolean {
  return (
    draft.secrets[name].trim() !== '' || (saved.includes(name) && !draft.cleared.includes(name))
  );
}

function settingsErrors(draft: DestinationDraft): DraftErrors {
  switch (draft.kind) {
    case AuditKind.Webhook:
      return draft.url.trim() === '' ? { [DestinationField.Url]: URL_REQUIRED } : {};
    case AuditKind.S3:
      return {
        ...(draft.bucket.trim() === '' ? { [DestinationField.Bucket]: BUCKET_REQUIRED } : {}),
        ...(draft.region.trim() === '' ? { [DestinationField.Region]: REGION_REQUIRED } : {}),
      };
    case AuditKind.Syslog: {
      const port = draft.port.trim();
      const number = Number(port);
      const valid = INTEGER.test(port) && number >= PORT_MINIMUM && number <= PORT_MAXIMUM;
      return {
        ...(draft.host.trim() === '' ? { [DestinationField.Host]: HOST_REQUIRED } : {}),
        ...(valid ? {} : { [DestinationField.Port]: refusalMessage(AuditCode.PortInvalid) }),
      };
    }
  }
}

function secretErrors(draft: DestinationDraft, saved: readonly SecretNameValue[]): DraftErrors {
  const required: readonly SecretNameValue[] = REQUIRED_SECRETS[draft.kind];
  const errors: DraftErrors = Object.fromEntries(
    required.filter((name) => !kept(draft, saved, name)).map((name) => [name, SECRET_REQUIRED])
  );
  const certificate = kept(draft, saved, SecretName.ClientCertificate);
  if (certificatesAllowed(draft) && certificate !== kept(draft, saved, SecretName.ClientKey)) {
    errors[certificate ? SecretName.ClientKey : SecretName.ClientCertificate] =
      CLIENT_PAIR_REQUIRED;
  }
  return errors;
}

export function draftErrors(
  draft: DestinationDraft,
  destination?: DestinationResponse
): DraftErrors {
  return {
    ...(draft.name.trim() === '' ? { [DestinationField.Name]: NAME_REQUIRED } : {}),
    ...settingsErrors(draft),
    ...secretErrors(draft, destination?.secrets ?? []),
  };
}

export function generateSecret(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(SECRET_BYTES));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}
