import { AuthRequestError } from '../../api/auth';
import { SsoCode } from './SsoCode';

export const SSO_INCOMPLETE = "Sign-in didn't complete. Start again from the sign-in page.";

export const SSO_MESSAGES: Record<SsoCode, string> = {
  [SsoCode.Required]:
    'This server uses single sign-on. Sign in with your identity provider; only server admins can use a password.',
  [SsoCode.AccountDisabled]:
    'This account has been turned off. Ask a server admin to turn it back on.',
  [SsoCode.StateInvalid]:
    'This sign-in expired or was already used. Start again from the sign-in page.',
  [SsoCode.ProviderNotFound]:
    'That sign-in option is no longer available. Choose another from the sign-in page.',
  [SsoCode.DiscoveryFailed]:
    "Zone couldn't reach your identity provider. Try again shortly, or ask a server admin to check the provider settings.",
  [SsoCode.TokenExchangeFailed]:
    "Your identity provider didn't accept Zone's sign-in request. Ask a server admin to check the client ID and secret.",
  [SsoCode.IdTokenInvalid]:
    "Zone couldn't verify the sign-in from your identity provider. Try again, or ask a server admin to check the provider settings.",
  [SsoCode.EmailMissing]:
    "Your identity provider didn't share an email address. Ask your administrator to release the email claim to Zone.",
  [SsoCode.EmailUnverified]:
    "Your identity provider hasn't verified your email address, so Zone can't sign you in with it.",
  [SsoCode.DomainNotAllowed]: "Your email domain isn't allowed to sign in to this server.",
  [SsoCode.EmailConflict]:
    'Your email address already belongs to a different Zone account. Ask a server admin for help.',
  [SsoCode.AssertionInvalid]:
    "Zone couldn't verify the sign-in from your identity provider. Try again, or ask a server admin to check the provider's metadata.",
  [SsoCode.SignInRefused]:
    "Your identity provider refused the sign-in. Check that you're assigned to Zone there.",
  [SsoCode.SignInFailed]:
    "Zone couldn't finish signing you in. Try again in a moment, or ask a server admin to check the server logs.",
};

function isSsoCode(code: string): code is SsoCode {
  return Object.hasOwn(SSO_MESSAGES, code);
}

export function ssoMessage(code: string): string | null {
  return isSsoCode(code) ? SSO_MESSAGES[code] : null;
}

export function messageOf(error: unknown, fallback: string): string {
  if (error instanceof AuthRequestError) {
    return ssoMessage(error.code) ?? error.message;
  }
  return error instanceof Error ? error.message : fallback;
}
