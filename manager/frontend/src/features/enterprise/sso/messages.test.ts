import { describe, expect, it } from 'bun:test';
import { ProviderField, refusalField, refusalMessage } from './messages';
import { SSO_ADMIN_CODES, SsoAdminCode } from './schemas';

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

  it('names Team and Enterprise when a license is required', () => {
    expect(refusalMessage(SsoAdminCode.LicenseRequired)).toContain('Team or Enterprise');
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
  };

  for (const code of SSO_ADMIN_CODES) {
    it(`puts ${code} on ${expected[code] ?? 'the whole form'}`, () => {
      expect(refusalField(code)).toBe(expected[code]);
    });
  }
});
