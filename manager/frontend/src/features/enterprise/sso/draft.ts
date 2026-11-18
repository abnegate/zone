import { ProviderField, refusalMessage } from './messages';
import { SsoAdminCode } from './schemas';
import type { CreateProviderRequest, ProviderResponse, UpdateProviderRequest } from './types';

export const DISPLAY_NAME_LIMIT = 64;

export const CLIENT_ID_LIMIT = 255;

export interface ProviderDraft {
  displayName: string;
  issuer: string;
  clientId: string;
  clientSecret: string;
  domains: string;
  requireVerifiedEmail: boolean;
  enabled: boolean;
}

export type DraftErrors = Partial<Record<ProviderField, string>>;

const DOMAIN_SEPARATOR = /[\s,]+/;

export function emptyDraft(): ProviderDraft {
  return {
    displayName: '',
    issuer: '',
    clientId: '',
    clientSecret: '',
    domains: '',
    requireVerifiedEmail: true,
    enabled: true,
  };
}

export function draftOf(provider: ProviderResponse): ProviderDraft {
  return {
    displayName: provider.display_name,
    issuer: provider.issuer,
    clientId: provider.client_id,
    clientSecret: '',
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

export function createRequest(draft: ProviderDraft): CreateProviderRequest {
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

export function updateRequest(
  provider: ProviderResponse,
  draft: ProviderDraft
): UpdateProviderRequest {
  const request: UpdateProviderRequest = {};
  const displayName = draft.displayName.trim();
  const issuer = draft.issuer.trim();
  const clientId = draft.clientId.trim();
  const domains = parseDomains(draft.domains);
  if (displayName !== provider.display_name) {
    request.display_name = displayName;
  }
  if (issuer !== provider.issuer) {
    request.issuer = issuer;
  }
  if (clientId !== provider.client_id) {
    request.client_id = clientId;
  }
  if (draft.clientSecret.trim() !== '') {
    request.client_secret = draft.clientSecret;
  }
  if (!sameDomains(domains, provider.allowed_domains)) {
    request.allowed_domains = domains;
  }
  if (draft.requireVerifiedEmail !== provider.require_verified_email) {
    request.require_verified_email = draft.requireVerifiedEmail;
  }
  if (draft.enabled !== provider.enabled) {
    request.enabled = draft.enabled;
  }
  return request;
}

export function changed(request: UpdateProviderRequest): boolean {
  return Object.keys(request).length > 0;
}

export function draftErrors(draft: ProviderDraft, provider?: ProviderResponse): DraftErrors {
  const errors: DraftErrors = {};
  const displayName = length(draft.displayName);
  if (displayName === 0 || displayName > DISPLAY_NAME_LIMIT) {
    errors[ProviderField.DisplayName] = refusalMessage(SsoAdminCode.DisplayNameInvalid);
  }
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
  return errors;
}

export function enabledCount(providers: readonly ProviderResponse[]): number {
  return providers.filter((provider) => provider.enabled).length;
}
