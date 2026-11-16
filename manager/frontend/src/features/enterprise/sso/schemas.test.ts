import { describe, expect, it } from 'bun:test';
import {
  COMMUNITY_EMPTY,
  ENFORCED,
  ENTITLED,
  ENTITLED_EMPTY,
  ENTRA,
  LAPSED_WITH_PROVIDER,
  OKTA,
} from './fixtures';
import {
  CreateProviderRequestSchema,
  EnforcementRequestSchema,
  EnforcementResponseSchema,
  ProviderResponseSchema,
  SettingsResponseSchema,
  SSO_ADMIN_CODES,
  SsoAdminCode,
  SsoRefusalSchema,
  UpdateProviderRequestSchema,
} from './schemas';

describe('SettingsResponseSchema', () => {
  for (const [name, settings] of [
    ['entitled', ENTITLED],
    ['entitled empty', ENTITLED_EMPTY],
    ['enforced', ENFORCED],
    ['Community', COMMUNITY_EMPTY],
    ['lapsed', LAPSED_WITH_PROVIDER],
  ] as const) {
    it(`reads the ${name} settings`, () => {
      expect(SettingsResponseSchema.parse(settings)).toEqual(settings);
    });
  }

  it('rejects settings without the redirect URI', () => {
    const { redirect_uri: _, ...settings } = ENTITLED;
    expect(SettingsResponseSchema.safeParse(settings).success).toBe(false);
  });

  it('rejects an edition the license does not know', () => {
    expect(SettingsResponseSchema.safeParse({ ...ENTITLED, edition: 'platinum' }).success).toBe(
      false
    );
  });
});

describe('ProviderResponseSchema', () => {
  for (const provider of [OKTA, ENTRA]) {
    it(`reads ${provider.display_name}`, () => {
      expect(ProviderResponseSchema.parse(provider)).toEqual(provider);
    });
  }

  it('rejects a protocol other than OIDC', () => {
    expect(ProviderResponseSchema.safeParse({ ...OKTA, protocol: 'saml' }).success).toBe(false);
  });

  it('drops a client secret the server should never send', () => {
    expect(ProviderResponseSchema.parse({ ...OKTA, client_secret: 's3cret' })).toEqual(OKTA);
  });
});

describe('CreateProviderRequestSchema', () => {
  const request = {
    display_name: ' Okta ',
    issuer: ' https://acme.okta.com ',
    client_id: ' 0oa1 ',
    client_secret: ' s3cret ',
  };

  it('trims the names and keeps the secret exactly as typed', () => {
    expect(CreateProviderRequestSchema.parse(request)).toEqual({
      display_name: 'Okta',
      issuer: 'https://acme.okta.com',
      client_id: '0oa1',
      client_secret: ' s3cret ',
    });
  });

  for (const field of ['display_name', 'issuer', 'client_id', 'client_secret'] as const) {
    it(`requires ${field}`, () => {
      expect(CreateProviderRequestSchema.safeParse({ ...request, [field]: '  ' }).success).toBe(
        field === 'client_secret'
      );
      const { [field]: _, ...missing } = request;
      expect(CreateProviderRequestSchema.safeParse(missing).success).toBe(false);
    });
  }
});

describe('UpdateProviderRequestSchema', () => {
  it('accepts an empty update', () => {
    expect(UpdateProviderRequestSchema.parse({})).toEqual({});
  });

  it('keeps only the fields it was given', () => {
    expect(UpdateProviderRequestSchema.parse({ enabled: false })).toEqual({ enabled: false });
  });

  it('rejects an empty secret, which would erase it', () => {
    expect(UpdateProviderRequestSchema.safeParse({ client_secret: '' }).success).toBe(false);
  });
});

describe('enforcement schemas', () => {
  it('reads and writes the required flag', () => {
    expect(EnforcementRequestSchema.parse({ required: true })).toEqual({ required: true });
    expect(EnforcementResponseSchema.parse({ required: false })).toEqual({ required: false });
    expect(EnforcementResponseSchema.safeParse({}).success).toBe(false);
  });
});

describe('SsoRefusalSchema', () => {
  it('lists every admin code once', () => {
    expect(SSO_ADMIN_CODES).toEqual(Object.values(SsoAdminCode));
    expect(new Set(SSO_ADMIN_CODES).size).toBe(SSO_ADMIN_CODES.length);
  });

  for (const code of SSO_ADMIN_CODES) {
    it(`reads a ${code} refusal`, () => {
      expect(SsoRefusalSchema.parse({ error: 'Refused', code })).toEqual({
        error: 'Refused',
        code,
      });
    });
  }

  it('rejects a code outside the contract', () => {
    expect(SsoRefusalSchema.safeParse({ error: 'Refused', code: 'sso_unheard_of' }).success).toBe(
      false
    );
  });
});
