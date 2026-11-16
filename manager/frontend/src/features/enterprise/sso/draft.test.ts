import { describe, expect, it } from 'bun:test';
import {
  CLIENT_ID_LIMIT,
  changed,
  createRequest,
  DISPLAY_NAME_LIMIT,
  draftErrors,
  draftOf,
  emptyDraft,
  enabledCount,
  parseDomains,
  updateRequest,
} from './draft';
import { ENTRA, OKTA } from './fixtures';
import { ProviderField, refusalMessage } from './messages';
import { SsoAdminCode } from './schemas';

const FILLED = {
  ...emptyDraft(),
  displayName: ' Okta ',
  issuer: ' https://acme.okta.com ',
  clientId: ' 0oa1 ',
  clientSecret: 's3cret',
};

describe('parseDomains', () => {
  it('splits on commas and spaces, lowercases, trims and drops repeats', () => {
    expect(parseDomains(' Acme.com, acme.co.uk ,,ACME.COM  example.org ')).toEqual([
      'acme.com',
      'acme.co.uk',
      'example.org',
    ]);
  });

  it('reads nothing from blank text', () => {
    expect(parseDomains(' , ')).toEqual([]);
  });
});

describe('emptyDraft', () => {
  it('requires verified emails and enables the provider by default', () => {
    expect(emptyDraft()).toMatchObject({ requireVerifiedEmail: true, enabled: true, domains: '' });
  });
});

describe('createRequest', () => {
  it('sends every field with trimmed names and normalized domains', () => {
    expect(createRequest({ ...FILLED, domains: 'Acme.com, acme.com', enabled: false })).toEqual({
      display_name: 'Okta',
      issuer: 'https://acme.okta.com',
      client_id: '0oa1',
      client_secret: 's3cret',
      allowed_domains: ['acme.com'],
      require_verified_email: true,
      enabled: false,
    });
  });
});

describe('updateRequest', () => {
  it('sends nothing when nothing changed and the secret is blank', () => {
    const request = updateRequest(OKTA, draftOf(OKTA));
    expect(request).toEqual({});
    expect(changed(request)).toBe(false);
  });

  it('sends only the changed fields', () => {
    expect(
      updateRequest(OKTA, { ...draftOf(OKTA), displayName: 'Okta SSO', enabled: false })
    ).toEqual({ display_name: 'Okta SSO', enabled: false });
  });

  it('ignores whitespace and domain case that the server would strip anyway', () => {
    expect(
      updateRequest(OKTA, {
        ...draftOf(OKTA),
        issuer: ` ${OKTA.issuer} `,
        domains: 'ACME.com',
      })
    ).toEqual({});
  });

  it('replaces the secret only when one is typed', () => {
    expect(updateRequest(OKTA, { ...draftOf(OKTA), clientSecret: '   ' })).toEqual({});
    expect(updateRequest(OKTA, { ...draftOf(OKTA), clientSecret: 'n3w' })).toEqual({
      client_secret: 'n3w',
    });
  });

  it('sends cleared domains as an empty list', () => {
    expect(updateRequest(OKTA, { ...draftOf(OKTA), domains: '' })).toEqual({
      allowed_domains: [],
    });
  });

  it('sends the verified-email switch when it flips', () => {
    expect(updateRequest(ENTRA, { ...draftOf(ENTRA), requireVerifiedEmail: true })).toEqual({
      require_verified_email: true,
    });
  });
});

describe('draftErrors', () => {
  it('accepts a complete new provider', () => {
    expect(draftErrors(FILLED)).toEqual({});
  });

  it('needs every field and a secret for a new provider', () => {
    expect(draftErrors(emptyDraft())).toEqual({
      [ProviderField.DisplayName]: refusalMessage(SsoAdminCode.DisplayNameInvalid),
      [ProviderField.Issuer]: refusalMessage(SsoAdminCode.IssuerInvalid),
      [ProviderField.ClientId]: refusalMessage(SsoAdminCode.ClientIdInvalid),
      [ProviderField.ClientSecret]: refusalMessage(SsoAdminCode.ClientSecretInvalid),
    });
  });

  it('keeps the stored secret when editing', () => {
    expect(draftErrors(draftOf(OKTA), OKTA)).toEqual({});
  });

  it('refuses names past their limits', () => {
    const errors = draftErrors({
      ...FILLED,
      displayName: 'n'.repeat(DISPLAY_NAME_LIMIT + 1),
      clientId: 'c'.repeat(CLIENT_ID_LIMIT + 1),
    });
    expect(Object.keys(errors)).toEqual([ProviderField.DisplayName, ProviderField.ClientId]);
  });

  it('counts characters, not code units, against the display name limit', () => {
    expect(draftErrors({ ...FILLED, displayName: '🔐'.repeat(DISPLAY_NAME_LIMIT) })).toEqual({});
  });
});

describe('enabledCount', () => {
  it('counts only enabled providers', () => {
    expect(enabledCount([OKTA, ENTRA])).toBe(1);
    expect(enabledCount([])).toBe(0);
  });
});
