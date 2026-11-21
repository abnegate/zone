import { METADATA_REQUIRED, ProviderField, refusalMessage } from './messages';
import { Protocol, SsoAdminCode } from './schemas';
import type {
  CreateProviderRequest,
  Protocol as ProtocolValue,
  ProviderResponse,
  UpdateProviderRequest,
} from './types';

export const DISPLAY_NAME_LIMIT = 64;

export const CLIENT_ID_LIMIT = 255;

export const MetadataSource = {
  Url: 'url',
  Xml: 'xml',
} as const;

export type MetadataSource = (typeof MetadataSource)[keyof typeof MetadataSource];

export interface ProviderDraft {
  protocol: ProtocolValue;
  displayName: string;
  issuer: string;
  clientId: string;
  clientSecret: string;
  metadataSource: MetadataSource;
  metadataUrl: string;
  metadata: string;
  refreshMetadata: boolean;
  domains: string;
  requireVerifiedEmail: boolean;
  enabled: boolean;
}

export type DraftErrors = Partial<Record<ProviderField, string>>;

const DOMAIN_SEPARATOR = /[\s,]+/;

export function emptyDraft(protocol: ProtocolValue = Protocol.Oidc): ProviderDraft {
  return {
    protocol,
    displayName: '',
    issuer: '',
    clientId: '',
    clientSecret: '',
    metadataSource: MetadataSource.Url,
    metadataUrl: '',
    metadata: '',
    refreshMetadata: false,
    domains: '',
    requireVerifiedEmail: protocol === Protocol.Oidc,
    enabled: true,
  };
}

export function draftOf(provider: ProviderResponse): ProviderDraft {
  return {
    protocol: provider.protocol,
    displayName: provider.display_name,
    issuer: provider.issuer,
    clientId: provider.client_id ?? '',
    clientSecret: '',
    metadataSource:
      provider.protocol === Protocol.Saml && provider.metadata_url === null
        ? MetadataSource.Xml
        : MetadataSource.Url,
    metadataUrl: provider.metadata_url ?? '',
    metadata: '',
    refreshMetadata: false,
    domains: provider.allowed_domains.join(', '),
    requireVerifiedEmail: provider.require_verified_email,
    enabled: provider.enabled,
  };
}

export function parseDomains(text: string): string[] {
  const domains = text
    .split(DOMAIN_SEPARATOR)
    .map((domain) => domain.trim().toLowerCase())
    .filter((domain) => domain !== '');
  return [...new Set(domains)];
}

function length(text: string): number {
  return [...text.trim()].length;
}

function sameDomains(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((domain, index) => domain === right[index]);
}

function chosenMetadata(draft: ProviderDraft): string {
  return (draft.metadataSource === MetadataSource.Url ? draft.metadataUrl : draft.metadata).trim();
}

function samlCreateRequest(draft: ProviderDraft): CreateProviderRequest {
  const source =
    draft.metadataSource === MetadataSource.Url
      ? { metadata_url: chosenMetadata(draft) }
      : { metadata: chosenMetadata(draft) };
  return {
    protocol: Protocol.Saml,
    display_name: draft.displayName.trim(),
    ...source,
    allowed_domains: parseDomains(draft.domains),
    enabled: draft.enabled,
  };
}

export function createRequest(draft: ProviderDraft): CreateProviderRequest {
  if (draft.protocol === Protocol.Saml) {
    return samlCreateRequest(draft);
  }
  return {
    display_name: draft.displayName.trim(),
    issuer: draft.issuer.trim(),
    client_id: draft.clientId.trim(),
    client_secret: draft.clientSecret,
    allowed_domains: parseDomains(draft.domains),
    require_verified_email: draft.requireVerifiedEmail,
    enabled: draft.enabled,
  };
}

function reviseOidc(
  request: UpdateProviderRequest,
  provider: ProviderResponse,
  draft: ProviderDraft
): void {
  const issuer = draft.issuer.trim();
  const clientId = draft.clientId.trim();
  if (issuer !== provider.issuer) {
    request.issuer = issuer;
  }
  if (clientId !== (provider.client_id ?? '')) {
    request.client_id = clientId;
  }
  if (draft.clientSecret.trim() !== '') {
    request.client_secret = draft.clientSecret;
  }
  if (draft.requireVerifiedEmail !== provider.require_verified_email) {
    request.require_verified_email = draft.requireVerifiedEmail;
  }
}

function reviseSaml(
  request: UpdateProviderRequest,
  provider: ProviderResponse,
  draft: ProviderDraft
): void {
  const metadata = chosenMetadata(draft);
  if (metadata === '') {
    return;
  }
  if (draft.metadataSource === MetadataSource.Xml) {
    request.metadata = metadata;
  } else if (metadata !== provider.metadata_url || draft.refreshMetadata) {
    request.metadata_url = metadata;
  }
}

export function updateRequest(
  provider: ProviderResponse,
  draft: ProviderDraft
): UpdateProviderRequest {
  const request: UpdateProviderRequest = {};
  const displayName = draft.displayName.trim();
  const domains = parseDomains(draft.domains);
  if (displayName !== provider.display_name) {
    request.display_name = displayName;
  }
  if (provider.protocol === Protocol.Saml) {
    reviseSaml(request, provider, draft);
  } else {
    reviseOidc(request, provider, draft);
  }
  if (!sameDomains(domains, provider.allowed_domains)) {
    request.allowed_domains = domains;
  }
  if (draft.enabled !== provider.enabled) {
    request.enabled = draft.enabled;
  }
  return request;
}

export function changed(request: UpdateProviderRequest): boolean {
  return Object.keys(request).length > 0;
}

function oidcErrors(errors: DraftErrors, draft: ProviderDraft, provider?: ProviderResponse): void {
  if (length(draft.issuer) === 0) {
    errors[ProviderField.Issuer] = refusalMessage(SsoAdminCode.IssuerInvalid);
  }
  const clientId = length(draft.clientId);
  if (clientId === 0 || clientId > CLIENT_ID_LIMIT) {
    errors[ProviderField.ClientId] = refusalMessage(SsoAdminCode.ClientIdInvalid);
  }
  if (provider === undefined && draft.clientSecret.trim() === '') {
    errors[ProviderField.ClientSecret] = refusalMessage(SsoAdminCode.ClientSecretInvalid);
  }
}

function samlErrors(errors: DraftErrors, draft: ProviderDraft, provider?: ProviderResponse): void {
  if (provider === undefined && chosenMetadata(draft) === '') {
    errors[ProviderField.Metadata] = METADATA_REQUIRED;
  }
  if (parseDomains(draft.domains).length === 0) {
    errors[ProviderField.AllowedDomains] = refusalMessage(SsoAdminCode.DomainRequired);
  }
}

export function draftErrors(draft: ProviderDraft, provider?: ProviderResponse): DraftErrors {
  const errors: DraftErrors = {};
  const displayName = length(draft.displayName);
  if (displayName === 0 || displayName > DISPLAY_NAME_LIMIT) {
    errors[ProviderField.DisplayName] = refusalMessage(SsoAdminCode.DisplayNameInvalid);
  }
  if (draft.protocol === Protocol.Saml) {
    samlErrors(errors, draft, provider);
  } else {
    oidcErrors(errors, draft, provider);
  }
  return errors;
}

export function enabledCount(providers: readonly ProviderResponse[]): number {
  return providers.filter((provider) => provider.enabled).length;
}
