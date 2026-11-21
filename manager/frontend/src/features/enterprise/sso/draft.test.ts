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
  MetadataSource,
  parseDomains,
  updateRequest,
} from './draft';
import { ENTRA, KEYCLOAK, OKTA, PASTED_SAML } from './fixtures';
import { METADATA_REQUIRED, ProviderField, refusalMessage } from './messages';
import { Protocol, SsoAdminCode } from './schemas';

const FILLED = {
  ...emptyDraft(),
  displayName: ' Okta ',
  issuer: ' https://acme.okta.com ',
  clientId: ' 0oa1 ',
  clientSecret: 's3cret',
};

const SAML = {
  ...emptyDraft(Protocol.Saml),
  displayName: ' Keycloak ',
  metadataUrl: ` ${KEYCLOAK.metadata_url} `,
  domains: 'Acme.com',
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
    expect(emptyDraft()).toMatchObject({
      protocol: Protocol.Oidc,
      requireVerifiedEmail: true,
      enabled: true,
      domains: '',
    });
  });

  it('starts SAML on a metadata URL without verified emails', () => {
    expect(emptyDraft(Protocol.Saml)).toMatchObject({
      protocol: Protocol.Saml,
      metadataSource: MetadataSource.Url,
      metadataUrl: '',
      metadata: '',
      requireVerifiedEmail: false,
    });
  });
});

describe('draftOf', () => {
  it('prefills the metadata URL of a SAML provider and leaves the XML empty', () => {
    expect(draftOf(KEYCLOAK)).toMatchObject({
      protocol: Protocol.Saml,
      clientId: '',
      metadataSource: MetadataSource.Url,
      metadataUrl: KEYCLOAK.metadata_url,
      metadata: '',
      refreshMetadata: false,
    });
  });

  it('opens a provider created from pasted XML on the XML source', () => {
    expect(draftOf(PASTED_SAML)).toMatchObject({
      metadataSource: MetadataSource.Xml,
      metadataUrl: '',
    });
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

describe('createRequest for SAML', () => {
  it('sends the metadata URL, the domains and the protocol, and nothing for OpenID Connect', () => {
    expect(createRequest(SAML)).toEqual({
      protocol: Protocol.Saml,
      display_name: 'Keycloak',
      metadata_url: KEYCLOAK.metadata_url,
      allowed_domains: ['acme.com'],
      enabled: true,
    });
  });

  it('sends the pasted XML instead when that is the source', () => {
    expect(
      createRequest({
        ...SAML,
        metadataSource: MetadataSource.Xml,
        metadata: ' <EntityDescriptor/> ',
      })
    ).toEqual({
      protocol: Protocol.Saml,
      display_name: 'Keycloak',
      metadata: '<EntityDescriptor/>',
      allowed_domains: ['acme.com'],
      enabled: true,
    });
  });

  it('never sends the source that is not chosen', () => {
    const request = createRequest({ ...SAML, metadata: '<EntityDescriptor/>' });
    expect(request).not.toHaveProperty('metadata');
    expect(request).toHaveProperty('metadata_url');
  });
});

describe('updateRequest for SAML', () => {
  it('sends nothing when nothing changed', () => {
    expect(updateRequest(KEYCLOAK, draftOf(KEYCLOAK))).toEqual({});
    expect(updateRequest(PASTED_SAML, draftOf(PASTED_SAML))).toEqual({});
  });

  it('sends only a changed metadata URL', () => {
    expect(
      updateRequest(KEYCLOAK, { ...draftOf(KEYCLOAK), metadataUrl: ' https://sso.acme.com/new ' })
    ).toEqual({ metadata_url: 'https://sso.acme.com/new' });
  });

  it('reads the same metadata URL again when asked to', () => {
    expect(updateRequest(KEYCLOAK, { ...draftOf(KEYCLOAK), refreshMetadata: true })).toEqual({
      metadata_url: KEYCLOAK.metadata_url,
    });
  });

  it('sends pasted XML alone, and keeps the metadata while the box is blank', () => {
    const xml = { ...draftOf(KEYCLOAK), metadataSource: MetadataSource.Xml };
    expect(updateRequest(KEYCLOAK, xml)).toEqual({});
    expect(updateRequest(KEYCLOAK, { ...xml, metadata: '<EntityDescriptor/>' })).toEqual({
      metadata: '<EntityDescriptor/>',
    });
  });

  it('never sends OpenID Connect fields', () => {
    expect(
      updateRequest(KEYCLOAK, {
        ...draftOf(KEYCLOAK),
        issuer: 'https://elsewhere.test',
        clientSecret: 's3cret',
        requireVerifiedEmail: true,
        enabled: false,
      })
    ).toEqual({ enabled: false });
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

describe('draftErrors for SAML', () => {
  it('accepts a complete new provider', () => {
    expect(draftErrors(SAML)).toEqual({});
  });

  it('needs a metadata source and a domain for a new provider, and no client fields', () => {
    expect(draftErrors(emptyDraft(Protocol.Saml))).toEqual({
      [ProviderField.DisplayName]: refusalMessage(SsoAdminCode.DisplayNameInvalid),
      [ProviderField.Metadata]: METADATA_REQUIRED,
      [ProviderField.AllowedDomains]: refusalMessage(SsoAdminCode.DomainRequired),
    });
  });

  it('checks only the chosen source', () => {
    expect(
      draftErrors({ ...SAML, metadataSource: MetadataSource.Xml, metadata: '' })
    ).toHaveProperty(ProviderField.Metadata);
    expect(
      draftErrors({ ...SAML, metadataSource: MetadataSource.Xml, metadata: '<EntityDescriptor/>' })
    ).toEqual({});
  });

  it('keeps the stored metadata when editing with a blank box', () => {
    expect(draftErrors({ ...draftOf(PASTED_SAML), metadata: '' }, PASTED_SAML)).toEqual({});
  });

  it('keeps requiring a domain when editing', () => {
    expect(draftErrors({ ...draftOf(KEYCLOAK), domains: ' , ' }, KEYCLOAK)).toEqual({
      [ProviderField.AllowedDomains]: refusalMessage(SsoAdminCode.DomainRequired),
    });
  });
});

describe('enabledCount', () => {
  it('counts only enabled providers', () => {
    expect(enabledCount([OKTA, ENTRA])).toBe(1);
    expect(enabledCount([])).toBe(0);
  });
});
