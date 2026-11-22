import { Protocol, SsoAdminCode } from './schemas';
import type { Protocol as ProtocolValue, SsoAdminCode as SsoAdminCodeValue } from './types';

export const UPGRADE_NOTICE =
  'Single sign-on is part of Zone Enterprise. Add a license to let people sign in with Okta, Microsoft Entra ID or Google Workspace.';

export const LAPSED_NOTICE =
  'Your Zone license has lapsed. Sign-in with these providers and the single sign-on requirement keep working, but changing them needs a renewed license.';

export const ProviderField = {
  DisplayName: 'display_name',
  Issuer: 'issuer',
  ClientId: 'client_id',
  ClientSecret: 'client_secret',
  Metadata: 'metadata',
  AllowedDomains: 'allowed_domains',
} as const;

export type ProviderField = (typeof ProviderField)[keyof typeof ProviderField];

export const METADATA_REQUIRED = 'Enter the metadata URL or paste the metadata XML.';

const PROTOCOL_NAMES: Readonly<Record<ProtocolValue, string>> = {
  [Protocol.Oidc]: 'OpenID Connect',
  [Protocol.Saml]: 'SAML 2.0',
};

const PROTOCOL_BADGES: Readonly<Record<ProtocolValue, string>> = {
  [Protocol.Oidc]: 'OIDC',
  [Protocol.Saml]: 'SAML',
};

const MESSAGES: Readonly<Record<SsoAdminCodeValue, string>> = {
  [SsoAdminCode.LicenseRequired]:
    'Changing single sign-on needs a Zone Enterprise license that is active or in its grace period.',
  [SsoAdminCode.ProviderNotFound]:
    'This provider no longer exists. It may have been deleted somewhere else.',
  [SsoAdminCode.ProviderRequired]:
    'Single sign-on is required, so at least one provider must stay enabled. Turn off "Require single sign-on" first.',
  [SsoAdminCode.ProviderDuplicate]:
    'A provider with this issuer and client ID, or this SAML entity ID, already exists.',
  [SsoAdminCode.DisplayNameInvalid]: 'Enter a display name of up to 64 characters.',
  [SsoAdminCode.IssuerInvalid]:
    'Enter the issuer as an https URL with no query or fragment, like https://acme.okta.com. Plain http works only for localhost.',
  [SsoAdminCode.ClientIdInvalid]: 'Enter the client ID, up to 255 characters.',
  [SsoAdminCode.ClientSecretInvalid]: 'Enter the client secret.',
  [SsoAdminCode.DomainInvalid]:
    'Enter domain names like acme.com, separated by commas, without @ or *.',
  [SsoAdminCode.DiscoveryFailed]: "Zone couldn't read this issuer's OpenID Connect configuration.",
  [SsoAdminCode.MetadataInvalid]: "Zone couldn't read this identity provider's SAML metadata.",
  [SsoAdminCode.DomainRequired]:
    "Enter at least one allowed domain. SAML doesn't say whether an email is verified, so Zone only accepts the domains you list.",
  [SsoAdminCode.VerifiedEmailUnsupported]:
    "SAML can't mark an email as verified, so a SAML provider can't require verified email.",
  [SsoAdminCode.ProtocolMismatch]:
    "That setting belongs to the other protocol. A provider's protocol can't change after it's added.",
};

const FIELDS: Readonly<Partial<Record<SsoAdminCodeValue, ProviderField>>> = {
  [SsoAdminCode.ProviderDuplicate]: ProviderField.ClientId,
  [SsoAdminCode.DisplayNameInvalid]: ProviderField.DisplayName,
  [SsoAdminCode.IssuerInvalid]: ProviderField.Issuer,
  [SsoAdminCode.ClientIdInvalid]: ProviderField.ClientId,
  [SsoAdminCode.ClientSecretInvalid]: ProviderField.ClientSecret,
  [SsoAdminCode.DomainInvalid]: ProviderField.AllowedDomains,
  [SsoAdminCode.DiscoveryFailed]: ProviderField.Issuer,
  [SsoAdminCode.MetadataInvalid]: ProviderField.Metadata,
  [SsoAdminCode.DomainRequired]: ProviderField.AllowedDomains,
};

const SAML_FIELDS: Readonly<Partial<Record<SsoAdminCodeValue, ProviderField>>> = {
  [SsoAdminCode.ProviderDuplicate]: ProviderField.Metadata,
};

const PROTOCOL_FIELDS: Readonly<Record<ProtocolValue, ReadonlySet<ProviderField>>> = {
  [Protocol.Oidc]: new Set([
    ProviderField.DisplayName,
    ProviderField.Issuer,
    ProviderField.ClientId,
    ProviderField.ClientSecret,
    ProviderField.AllowedDomains,
  ]),
  [Protocol.Saml]: new Set([
    ProviderField.DisplayName,
    ProviderField.Metadata,
    ProviderField.AllowedDomains,
  ]),
};

const DETAILED: ReadonlySet<SsoAdminCodeValue> = new Set([
  SsoAdminCode.DiscoveryFailed,
  SsoAdminCode.MetadataInvalid,
]);

export function protocolName(protocol: ProtocolValue): string {
  return PROTOCOL_NAMES[protocol];
}

export function protocolBadge(protocol: ProtocolValue): string {
  return PROTOCOL_BADGES[protocol];
}

export function refusalMessage(code: SsoAdminCodeValue, detail = ''): string {
  const message = MESSAGES[code];
  const reason = detail.trim();
  return DETAILED.has(code) && reason !== '' && reason !== message
    ? `${message} ${reason}`
    : message;
}

export function refusalField(
  code: SsoAdminCodeValue,
  protocol: ProtocolValue = Protocol.Oidc
): ProviderField | null {
  const field = (protocol === Protocol.Saml ? SAML_FIELDS[code] : undefined) ?? FIELDS[code];
  return field !== undefined && PROTOCOL_FIELDS[protocol].has(field) ? field : null;
}
