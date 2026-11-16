import { SsoAdminCode } from './schemas';
import type { SsoAdminCode as SsoAdminCodeValue } from './types';

export const ProviderField = {
  DisplayName: 'display_name',
  Issuer: 'issuer',
  ClientId: 'client_id',
  ClientSecret: 'client_secret',
  AllowedDomains: 'allowed_domains',
} as const;

export type ProviderField = (typeof ProviderField)[keyof typeof ProviderField];

const MESSAGES: Readonly<Record<SsoAdminCodeValue, string>> = {
  [SsoAdminCode.LicenseRequired]:
    'Changing single sign-on needs an active Zone Team or Enterprise license.',
  [SsoAdminCode.ProviderNotFound]:
    'This provider no longer exists. It may have been deleted somewhere else.',
  [SsoAdminCode.ProviderRequired]:
    'Single sign-on is required, so at least one provider must stay enabled. Turn off "Require single sign-on" first.',
  [SsoAdminCode.ProviderDuplicate]: 'A provider with this issuer and client ID already exists.',
  [SsoAdminCode.DisplayNameInvalid]: 'Enter a display name of up to 64 characters.',
  [SsoAdminCode.IssuerInvalid]:
    'Enter the issuer as an https URL with no query or fragment, like https://acme.okta.com. Plain http works only for localhost.',
  [SsoAdminCode.ClientIdInvalid]: 'Enter the client ID, up to 255 characters.',
  [SsoAdminCode.ClientSecretInvalid]: 'Enter the client secret.',
  [SsoAdminCode.DomainInvalid]:
    'Enter domain names like acme.com, separated by commas, without @ or *.',
  [SsoAdminCode.DiscoveryFailed]: "Zone couldn't read this issuer's OpenID Connect configuration.",
};

const FIELDS: Readonly<Partial<Record<SsoAdminCodeValue, ProviderField>>> = {
  [SsoAdminCode.ProviderDuplicate]: ProviderField.ClientId,
  [SsoAdminCode.DisplayNameInvalid]: ProviderField.DisplayName,
  [SsoAdminCode.IssuerInvalid]: ProviderField.Issuer,
  [SsoAdminCode.ClientIdInvalid]: ProviderField.ClientId,
  [SsoAdminCode.ClientSecretInvalid]: ProviderField.ClientSecret,
  [SsoAdminCode.DomainInvalid]: ProviderField.AllowedDomains,
  [SsoAdminCode.DiscoveryFailed]: ProviderField.Issuer,
};

const DETAILED: ReadonlySet<SsoAdminCodeValue> = new Set([SsoAdminCode.DiscoveryFailed]);

export function refusalMessage(code: SsoAdminCodeValue, detail = ''): string {
  const message = MESSAGES[code];
  const reason = detail.trim();
  return DETAILED.has(code) && reason !== '' && reason !== message
    ? `${message} ${reason}`
    : message;
}

export function refusalField(code: SsoAdminCodeValue): ProviderField | null {
  return FIELDS[code] ?? null;
}
