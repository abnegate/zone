import type { SsoPublicProvider, SsoPublicProviders } from './types';

export const TEST_ORIGIN = 'http://localhost:3000';

export const OKTA: SsoPublicProvider = {
  id: '3f0c8a52-6d1e-4b7a-9c2f-1e5d7a9b0c41',
  display_name: 'Okta',
};

export const ENTRA: SsoPublicProvider = {
  id: '9b2e4d61-0a3c-4f8e-b7d5-6c1a2e3f4b50',
  display_name: 'Microsoft Entra ID',
};

export const NO_SSO: SsoPublicProviders = {
  required: false,
  callback_origin: TEST_ORIGIN,
  providers: [],
};

export const SSO_OFFERED: SsoPublicProviders = {
  required: false,
  callback_origin: TEST_ORIGIN,
  providers: [OKTA, ENTRA],
};

export const SSO_REQUIRED: SsoPublicProviders = {
  required: true,
  callback_origin: TEST_ORIGIN,
  providers: [OKTA],
};

export function atOrigin(providers: SsoPublicProviders, origin: string): SsoPublicProviders {
  return { ...providers, callback_origin: origin };
}
