export const SsoCode = {
  Required: 'sso_required',
  AccountDisabled: 'account_disabled',
  StateInvalid: 'sso_state_invalid',
  ProviderNotFound: 'sso_provider_not_found',
  DiscoveryFailed: 'sso_discovery_failed',
  TokenExchangeFailed: 'sso_token_exchange_failed',
  IdTokenInvalid: 'sso_id_token_invalid',
  EmailMissing: 'sso_email_missing',
  EmailUnverified: 'sso_email_unverified',
  DomainNotAllowed: 'sso_domain_not_allowed',
  EmailConflict: 'sso_email_conflict',
} as const;

export type SsoCode = (typeof SsoCode)[keyof typeof SsoCode];
