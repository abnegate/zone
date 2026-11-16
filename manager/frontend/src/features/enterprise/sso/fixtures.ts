import { Edition, LicenseStatus } from '../schemas';
import { Protocol } from './schemas';
import type { ProviderResponse, SettingsResponse } from './types';

export const REDIRECT_URI = 'https://zone.example.com/sso/callback';

export const OKTA: ProviderResponse = {
  id: '3f2a1b4c-5d6e-4f70-8a9b-0c1d2e3f4a5b',
  protocol: Protocol.Oidc,
  display_name: 'Okta',
  issuer: 'https://acme.okta.com',
  client_id: '0oa1b2c3d4',
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
  allowed_domains: [],
  require_verified_email: false,
  enabled: false,
  created_at: '2026-11-11T09:00:00Z',
  updated_at: '2026-11-12T09:00:00Z',
};

export const ENTITLED: SettingsResponse = {
  entitled: true,
  edition: Edition.Team,
  status: LicenseStatus.Active,
  required: false,
  redirect_uri: REDIRECT_URI,
  providers: [OKTA, ENTRA],
};

export const ENTITLED_EMPTY: SettingsResponse = {
  ...ENTITLED,
  providers: [],
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
  providers: [],
};

export const LAPSED_WITH_PROVIDER: SettingsResponse = {
  entitled: false,
  edition: Edition.Team,
  status: LicenseStatus.Lapsed,
  required: true,
  redirect_uri: REDIRECT_URI,
  providers: [OKTA],
};
