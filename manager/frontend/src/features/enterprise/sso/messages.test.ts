import { describe, expect, it } from 'bun:test';
import {
  ProviderField,
  protocolBadge,
  protocolName,
  refusalField,
  refusalMessage,
} from './messages';
import { PROTOCOLS, Protocol, SSO_ADMIN_CODES, SsoAdminCode } from './schemas';

describe('refusalMessage', () => {
  for (const code of SSO_ADMIN_CODES) {
    it(`explains ${code} in a sentence`, () => {
      const message = refusalMessage(code);
      expect(message.length).toBeGreaterThan(0);
      expect(message).toEndWith('.');
    });
  }

  it('gives every code its own message', () => {
    const messages = SSO_ADMIN_CODES.map((code) => refusalMessage(code));
    expect(new Set(messages).size).toBe(SSO_ADMIN_CODES.length);
  });

  it('adds the server reason to a discovery failure', () => {
    expect(
      refusalMessage(SsoAdminCode.DiscoveryFailed, 'The issuer in the document does not match.')
    ).toBe(
      "Zone couldn't read this issuer's OpenID Connect configuration. The issuer in the document does not match."
    );
  });

  it('adds the server reason to a metadata failure', () => {
    expect(
      refusalMessage(SsoAdminCode.MetadataInvalid, 'The metadata has no signing certificate.')
    ).toBe(
      "Zone couldn't read this identity provider's SAML metadata. The metadata has no signing certificate."
    );
  });

  it('explains why SAML needs a domain and never a verified email', () => {
    expect(refusalMessage(SsoAdminCode.DomainRequired)).toContain('allowed domain');
    expect(refusalMessage(SsoAdminCode.VerifiedEmailUnsupported)).toContain('verified email');
  });

  it('keeps a discovery failure on its own message without a reason', () => {
    expect(refusalMessage(SsoAdminCode.DiscoveryFailed, '  ')).toBe(
      refusalMessage(SsoAdminCode.DiscoveryFailed)
    );
  });

  it('leaves the server text off codes that explain themselves', () => {
    expect(refusalMessage(SsoAdminCode.IssuerInvalid, 'raw server text')).not.toContain(
      'raw server text'
    );
  });

  it('names Zone Enterprise when a license is required', () => {
    expect(refusalMessage(SsoAdminCode.LicenseRequired)).toBe(
      'Changing single sign-on needs a Zone Enterprise license that is active or in its grace period.'
    );
  });
});

describe('refusalField', () => {
  const expected = {
    [SsoAdminCode.LicenseRequired]: null,
    [SsoAdminCode.ProviderNotFound]: null,
    [SsoAdminCode.ProviderRequired]: null,
    [SsoAdminCode.ProviderDuplicate]: ProviderField.ClientId,
    [SsoAdminCode.DisplayNameInvalid]: ProviderField.DisplayName,
    [SsoAdminCode.IssuerInvalid]: ProviderField.Issuer,
    [SsoAdminCode.ClientIdInvalid]: ProviderField.ClientId,
    [SsoAdminCode.ClientSecretInvalid]: ProviderField.ClientSecret,
    [SsoAdminCode.DomainInvalid]: ProviderField.AllowedDomains,
    [SsoAdminCode.DiscoveryFailed]: ProviderField.Issuer,
    [SsoAdminCode.MetadataInvalid]: null,
    [SsoAdminCode.DomainRequired]: ProviderField.AllowedDomains,
    [SsoAdminCode.VerifiedEmailUnsupported]: null,
    [SsoAdminCode.ProtocolMismatch]: null,
  };

  const saml = {
    ...expected,
    [SsoAdminCode.ProviderDuplicate]: ProviderField.Metadata,
    [SsoAdminCode.IssuerInvalid]: null,
    [SsoAdminCode.ClientIdInvalid]: null,
    [SsoAdminCode.ClientSecretInvalid]: null,
    [SsoAdminCode.DiscoveryFailed]: null,
    [SsoAdminCode.MetadataInvalid]: ProviderField.Metadata,
  };

  for (const code of SSO_ADMIN_CODES) {
    it(`puts ${code} on ${expected[code] ?? 'the whole form'} for OpenID Connect`, () => {
      expect(refusalField(code)).toBe(expected[code]);
      expect(refusalField(code, Protocol.Oidc)).toBe(expected[code]);
    });

    it(`puts ${code} on ${saml[code] ?? 'the whole form'} for SAML`, () => {
      expect(refusalField(code, Protocol.Saml)).toBe(saml[code]);
    });
  }
});

describe('protocol names', () => {
  it('names each protocol in full and as a badge', () => {
    expect(PROTOCOLS.map(protocolName)).toEqual(['OpenID Connect', 'SAML 2.0']);
    expect(PROTOCOLS.map(protocolBadge)).toEqual(['OIDC', 'SAML']);
  });
});
