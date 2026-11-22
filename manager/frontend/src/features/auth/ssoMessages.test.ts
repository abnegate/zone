import { describe, expect, it } from 'bun:test';
import { AuthRequestError } from '../../api/auth';
import { memberCapMessage } from '../enterprise/members';
import { SsoCode } from './SsoCode';
import { messageOf, SSO_INCOMPLETE, SSO_MESSAGES, ssoMessage } from './ssoMessages';

const CALLBACK_CODES = [
  'sso_state_invalid',
  'sso_provider_not_found',
  'sso_discovery_failed',
  'sso_token_exchange_failed',
  'sso_id_token_invalid',
  'sso_email_missing',
  'sso_email_unverified',
  'sso_domain_not_allowed',
  'sso_email_conflict',
  'sso_assertion_invalid',
  'sso_sign_in_refused',
  'sso_sign_in_failed',
  'account_disabled',
];

describe('ssoMessages', () => {
  it('covers every callback code plus sso_required and account_disabled', () => {
    expect(Object.values(SsoCode).sort()).toEqual([...CALLBACK_CODES, 'sso_required'].sort());
    for (const code of Object.values(SsoCode)) {
      expect(SSO_MESSAGES[code].length).toBeGreaterThan(0);
    }
  });

  it('gives each code its own message', () => {
    const messages = Object.values(SSO_MESSAGES);
    expect(new Set(messages).size).toBe(messages.length);
  });

  it('writes messages as plain sentences', () => {
    for (const message of [...Object.values(SSO_MESSAGES), SSO_INCOMPLETE]) {
      expect(message).toMatch(/^[A-Z].*\.$/);
      expect(message).not.toMatch(/[<>\n]/);
    }
  });

  it('pins the message shown when a callback cannot be trusted', () => {
    expect(SSO_INCOMPLETE).toBe("Sign-in didn't complete. Start again from the sign-in page.");
  });

  it('pins the SAML sign-in messages', () => {
    expect(SSO_MESSAGES[SsoCode.AssertionInvalid]).toBe(
      "Zone couldn't verify the sign-in from your identity provider. Try again, or ask a server admin to check the provider's metadata."
    );
    expect(SSO_MESSAGES[SsoCode.SignInRefused]).toBe(
      "Your identity provider refused the sign-in. Check that you're assigned to Zone there."
    );
    expect(SSO_MESSAGES[SsoCode.SignInFailed]).toBe(
      "Zone couldn't finish signing you in. Try again in a moment, or ask a server admin to check the server logs."
    );
  });

  it('looks up a known code and returns null for anything else', () => {
    expect(ssoMessage('sso_required')).toBe(SSO_MESSAGES[SsoCode.Required]);
    expect(ssoMessage('device_pending')).toBeNull();
    expect(ssoMessage('constructor')).toBeNull();
    expect(ssoMessage('')).toBeNull();
  });

  describe('messageOf', () => {
    it('uses the message for a known refusal code', () => {
      const error = new AuthRequestError('This server requires single sign-on.', 'sso_required');
      expect(messageOf(error, 'Login failed')).toBe(SSO_MESSAGES[SsoCode.Required]);
    });

    it("keeps the server's message for a code it does not know", () => {
      const error = new AuthRequestError('This device is waiting for an admin', 'device_pending');
      expect(messageOf(error, 'Login failed')).toBe('This device is waiting for an admin');
    });

    it('explains the member cap for its refusal code instead of the server text', () => {
      const error = new AuthRequestError('Server text', 'member_cap_reached');
      expect(messageOf(error, 'Registration failed')).toBe(memberCapMessage());
    });

    it('keeps the member cap out of the single sign-on messages', () => {
      expect(ssoMessage('member_cap_reached')).toBeNull();
      expect(Object.values(SSO_MESSAGES)).not.toContain(memberCapMessage());
    });

    it('uses the message of a plain error', () => {
      expect(messageOf(new Error('Invalid credentials'), 'Login failed')).toBe(
        'Invalid credentials'
      );
    });

    it('falls back for anything that is not an error', () => {
      expect(messageOf('boom', 'Login failed')).toBe('Login failed');
    });
  });
});
