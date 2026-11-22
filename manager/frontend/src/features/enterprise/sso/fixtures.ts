import { Edition, LicenseStatus } from '../schemas';
import { Protocol } from './schemas';
import type { ProviderResponse, ServiceProviderResponse, SettingsResponse } from './types';

export const REDIRECT_URI = 'https://zone.example.com/sso/callback';

export const SERVICE_PROVIDER: ServiceProviderResponse = {
  entity_id: 'https://zone.example.com/api/auth/sso/saml/metadata',
  acs_url: 'https://zone.example.com/api/auth/sso/saml/acs',
  metadata_url: 'https://zone.example.com/api/auth/sso/saml/metadata',
};

export const OKTA: ProviderResponse = {
  id: '3f2a1b4c-5d6e-4f70-8a9b-0c1d2e3f4a5b',
  protocol: Protocol.Oidc,
  display_name: 'Okta',
  issuer: 'https://acme.okta.com',
  client_id: '0oa1b2c3d4',
  sso_url: null,
  metadata_url: null,
  certificate_fingerprints: [],
  allowed_domains: ['acme.com'],
  require_verified_email: true,
  enabled: true,
  created_at: '2026-11-10T09:00:00Z',
  updated_at: '2026-11-10T09:00:00Z',
};

export const ENTRA: ProviderResponse = {
  id: '7b8c9d0e-1f2a-4b3c-9d4e-5f6a7b8c9d0e',
  protocol: Protocol.Oidc,
  display_name: 'Microsoft Entra ID',
  issuer: 'https://login.microsoftonline.com/4a5b6c7d-8e9f-4a0b-1c2d-3e4f5a6b7c8d/v2.0',
  client_id: '11111111-2222-4333-8444-555555555555',
  sso_url: null,
  metadata_url: null,
  certificate_fingerprints: [],
  allowed_domains: [],
  require_verified_email: false,
  enabled: false,
  created_at: '2026-11-11T09:00:00Z',
  updated_at: '2026-11-12T09:00:00Z',
};

export const KEYCLOAK: ProviderResponse = {
  id: 'c4d5e6f7-0a1b-4c2d-8e3f-4a5b6c7d8e9f',
  protocol: Protocol.Saml,
  display_name: 'Keycloak',
  issuer: 'https://sso.acme.com/realms/acme',
  client_id: null,
  sso_url: 'https://sso.acme.com/realms/acme/protocol/saml',
  metadata_url: 'https://sso.acme.com/realms/acme/protocol/saml/descriptor',
  certificate_fingerprints: [
    '3A:91:5C:0E:7B:22:D4:6F:18:A0:C3:5E:9B:41:F7:02:6D:8C:E5:13:B9:70:4A:2F:C6:58:0D:E1:97:3B:AC:64',
    'F0:12:8E:4D:A7:39:C1:56:0B:E2:7F:94:3D:68:B5:21:CA:0F:73:9E:46:D8:15:B2:6A:E0:3C:87:51:F9:2B:D4',
  ],
  allowed_domains: ['acme.com'],
  require_verified_email: false,
  enabled: true,
  created_at: '2026-11-20T09:00:00Z',
  updated_at: '2026-11-20T09:00:00Z',
};

export const PASTED_SAML: ProviderResponse = {
  ...KEYCLOAK,
  id: 'd5e6f7a8-1b2c-4d3e-9f4a-5b6c7d8e9f0a',
  display_name: 'ADFS',
  issuer: 'http://adfs.acme.com/adfs/services/trust',
  sso_url: 'https://adfs.acme.com/adfs/ls/',
  metadata_url: null,
  certificate_fingerprints: [KEYCLOAK.certificate_fingerprints[0]],
};

export const ENTITLED: SettingsResponse = {
  entitled: true,
  edition: Edition.Enterprise,
  status: LicenseStatus.Active,
  required: false,
  redirect_uri: REDIRECT_URI,
  service_provider: SERVICE_PROVIDER,
  providers: [OKTA, ENTRA],
};

export const ENTITLED_EMPTY: SettingsResponse = {
  ...ENTITLED,
  providers: [],
};

export const ENTITLED_WITH_SAML: SettingsResponse = {
  ...ENTITLED,
  providers: [OKTA, KEYCLOAK],
};

export const ENFORCED: SettingsResponse = {
  ...ENTITLED,
  edition: Edition.Enterprise,
  required: true,
  providers: [OKTA],
};

export const COMMUNITY_EMPTY: SettingsResponse = {
  entitled: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
  required: false,
  redirect_uri: REDIRECT_URI,
  service_provider: SERVICE_PROVIDER,
  providers: [],
};

export const LAPSED_WITH_PROVIDER: SettingsResponse = {
  entitled: false,
  edition: Edition.Enterprise,
  status: LicenseStatus.Lapsed,
  required: true,
  redirect_uri: REDIRECT_URI,
  service_provider: SERVICE_PROVIDER,
  providers: [OKTA],
};

export const TEAM_ACTIVE: SettingsResponse = {
  ...COMMUNITY_EMPTY,
  edition: Edition.Team,
  status: LicenseStatus.Active,
};

export const TEAM_LAPSED: SettingsResponse = {
  ...COMMUNITY_EMPTY,
  edition: Edition.Team,
  status: LicenseStatus.Lapsed,
  providers: [OKTA],
};
