use crate::sso::account::ACCOUNT_DISABLED;
use crate::sso::rejection::Rejection;
use crate::sso::saml::Refusal;

pub const SSO_REQUIRED: &str = "sso_required";
pub const PROVIDER_NOT_FOUND: &str = "sso_provider_not_found";
pub const PROVIDER_REQUIRED: &str = "sso_provider_required";
pub const PROVIDER_DUPLICATE: &str = "sso_provider_duplicate";
pub const DISPLAY_NAME_INVALID: &str = "sso_display_name_invalid";
pub const ISSUER_INVALID: &str = "sso_issuer_invalid";
pub const CLIENT_ID_INVALID: &str = "sso_client_id_invalid";
pub const CLIENT_SECRET_INVALID: &str = "sso_client_secret_invalid";
pub const DOMAIN_INVALID: &str = "sso_domain_invalid";
pub const DISCOVERY_FAILED: &str = "sso_discovery_failed";
pub const STATE_INVALID: &str = "sso_state_invalid";
pub const TOKEN_EXCHANGE_FAILED: &str = "sso_token_exchange_failed";
pub const ID_TOKEN_INVALID: &str = "sso_id_token_invalid";
pub const EMAIL_MISSING: &str = "sso_email_missing";
pub const EMAIL_UNVERIFIED: &str = "sso_email_unverified";
pub const DOMAIN_NOT_ALLOWED: &str = "sso_domain_not_allowed";
pub const EMAIL_CONFLICT: &str = "sso_email_conflict";
pub const METADATA_INVALID: &str = "sso_metadata_invalid";
pub const ASSERTION_INVALID: &str = "sso_assertion_invalid";
pub const SIGN_IN_REFUSED: &str = "sso_sign_in_refused";
pub const SIGN_IN_FAILED: &str = "sso_sign_in_failed";
pub const DOMAIN_REQUIRED: &str = "sso_domain_required";
pub const VERIFIED_EMAIL_UNSUPPORTED: &str = "sso_verified_email_unsupported";
pub const PROTOCOL_MISMATCH: &str = "sso_protocol_mismatch";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error(
        "This server requires single sign-on. Sign in from the Zone console in your browser; only server admins can sign in with a password."
    )]
    Required,
    #[error("That single sign-on provider doesn't exist or is turned off.")]
    ProviderNotFound,
    #[error("Single sign-on is required, so at least one provider must stay enabled.")]
    ProviderRequired,
    #[error("A provider with this issuer and client ID already exists.")]
    ProviderDuplicate,
    #[error("The display name must be between 1 and 64 characters.")]
    DisplayNameInvalid,
    #[error(
        "The issuer must be an absolute https URL with no query or fragment; plain http is only allowed for localhost."
    )]
    IssuerInvalid,
    #[error("The client ID must be between 1 and 255 characters.")]
    ClientIdInvalid,
    #[error("The client secret can't be empty.")]
    ClientSecretInvalid,
    #[error("\"{0}\" is not a valid domain. Use a domain name such as acme.com.")]
    DomainInvalid(String),
    #[error("The identity provider's configuration couldn't be used: {0}")]
    Discovery(String),
    #[error("This sign-in has expired or was already used. Start again from the sign-in page.")]
    StateInvalid,
    #[error("The identity provider didn't accept the sign-in: {0}")]
    TokenExchange(String),
    #[error("The identity provider's ID token was refused because {0}.")]
    IdTokenInvalid(Rejection),
    #[error("The identity provider didn't share an email address.")]
    EmailMissing,
    #[error("The identity provider hasn't verified this email address.")]
    EmailUnverified,
    #[error("This email domain isn't allowed to sign in to this server.")]
    DomainNotAllowed,
    #[error(
        "This email address belongs to an account that can't be linked to this sign-in. Ask a server admin for help."
    )]
    EmailConflict,
    #[error("This account is disabled.")]
    AccountDisabled,
    #[error("The identity provider's SAML metadata couldn't be used: {0}")]
    MetadataInvalid(String),
    #[error("The identity provider's SAML response was refused because {0}.")]
    AssertionInvalid(Refusal),
    #[error("The identity provider refused the sign-in with status {0}.")]
    SignInRefused(String),
    #[error("A SAML provider needs at least one allowed email domain.")]
    DomainRequired,
    #[error(
        "A SAML provider can't require a verified email, because SAML doesn't say whether the identity provider verified it."
    )]
    VerifiedEmailUnsupported,
    #[error("That setting belongs to a different single sign-on protocol than this provider's.")]
    ProtocolMismatch,
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Required => SSO_REQUIRED,
            Self::ProviderNotFound => PROVIDER_NOT_FOUND,
            Self::ProviderRequired => PROVIDER_REQUIRED,
            Self::ProviderDuplicate => PROVIDER_DUPLICATE,
            Self::DisplayNameInvalid => DISPLAY_NAME_INVALID,
            Self::IssuerInvalid => ISSUER_INVALID,
            Self::ClientIdInvalid => CLIENT_ID_INVALID,
            Self::ClientSecretInvalid => CLIENT_SECRET_INVALID,
            Self::DomainInvalid(_) => DOMAIN_INVALID,
            Self::Discovery(_) => DISCOVERY_FAILED,
            Self::StateInvalid => STATE_INVALID,
            Self::TokenExchange(_) => TOKEN_EXCHANGE_FAILED,
            Self::IdTokenInvalid(_) => ID_TOKEN_INVALID,
            Self::EmailMissing => EMAIL_MISSING,
            Self::EmailUnverified => EMAIL_UNVERIFIED,
            Self::DomainNotAllowed => DOMAIN_NOT_ALLOWED,
            Self::EmailConflict => EMAIL_CONFLICT,
            Self::AccountDisabled => ACCOUNT_DISABLED,
            Self::MetadataInvalid(_) => METADATA_INVALID,
            Self::AssertionInvalid(_) => ASSERTION_INVALID,
            Self::SignInRefused(_) => SIGN_IN_REFUSED,
            Self::DomainRequired => DOMAIN_REQUIRED,
            Self::VerifiedEmailUnsupported => VERIFIED_EMAIL_UNSUPPORTED,
            Self::ProtocolMismatch => PROTOCOL_MISMATCH,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every() -> [Error; 24] {
        [
            Error::Required,
            Error::ProviderNotFound,
            Error::ProviderRequired,
            Error::ProviderDuplicate,
            Error::DisplayNameInvalid,
            Error::IssuerInvalid,
            Error::ClientIdInvalid,
            Error::ClientSecretInvalid,
            Error::DomainInvalid("acme_com".into()),
            Error::Discovery("unreachable".into()),
            Error::StateInvalid,
            Error::TokenExchange("HTTP 400".into()),
            Error::IdTokenInvalid(Rejection::Signature),
            Error::EmailMissing,
            Error::EmailUnverified,
            Error::DomainNotAllowed,
            Error::EmailConflict,
            Error::AccountDisabled,
            Error::MetadataInvalid("no signing certificate".into()),
            Error::AssertionInvalid(Refusal::Unsigned),
            Error::SignInRefused("urn:oasis:names:tc:SAML:2.0:status:Responder".into()),
            Error::DomainRequired,
            Error::VerifiedEmailUnsupported,
            Error::ProtocolMismatch,
        ]
    }

    #[test]
    fn codes_are_the_wire_contract() {
        let codes: Vec<&str> = every().iter().map(Error::code).collect();
        assert_eq!(
            codes,
            [
                "sso_required",
                "sso_provider_not_found",
                "sso_provider_required",
                "sso_provider_duplicate",
                "sso_display_name_invalid",
                "sso_issuer_invalid",
                "sso_client_id_invalid",
                "sso_client_secret_invalid",
                "sso_domain_invalid",
                "sso_discovery_failed",
                "sso_state_invalid",
                "sso_token_exchange_failed",
                "sso_id_token_invalid",
                "sso_email_missing",
                "sso_email_unverified",
                "sso_domain_not_allowed",
                "sso_email_conflict",
                "account_disabled",
                "sso_metadata_invalid",
                "sso_assertion_invalid",
                "sso_sign_in_refused",
                "sso_domain_required",
                "sso_verified_email_unsupported",
                "sso_protocol_mismatch",
            ]
        );
    }

    #[test]
    fn constants_are_the_wire_contract() {
        assert_eq!(SSO_REQUIRED, "sso_required");
        assert_eq!(PROVIDER_NOT_FOUND, "sso_provider_not_found");
        assert_eq!(PROVIDER_REQUIRED, "sso_provider_required");
        assert_eq!(PROVIDER_DUPLICATE, "sso_provider_duplicate");
        assert_eq!(DISPLAY_NAME_INVALID, "sso_display_name_invalid");
        assert_eq!(ISSUER_INVALID, "sso_issuer_invalid");
        assert_eq!(CLIENT_ID_INVALID, "sso_client_id_invalid");
        assert_eq!(CLIENT_SECRET_INVALID, "sso_client_secret_invalid");
        assert_eq!(DOMAIN_INVALID, "sso_domain_invalid");
        assert_eq!(DISCOVERY_FAILED, "sso_discovery_failed");
        assert_eq!(STATE_INVALID, "sso_state_invalid");
        assert_eq!(TOKEN_EXCHANGE_FAILED, "sso_token_exchange_failed");
        assert_eq!(ID_TOKEN_INVALID, "sso_id_token_invalid");
        assert_eq!(EMAIL_MISSING, "sso_email_missing");
        assert_eq!(EMAIL_UNVERIFIED, "sso_email_unverified");
        assert_eq!(DOMAIN_NOT_ALLOWED, "sso_domain_not_allowed");
        assert_eq!(EMAIL_CONFLICT, "sso_email_conflict");
        assert_eq!(METADATA_INVALID, "sso_metadata_invalid");
        assert_eq!(ASSERTION_INVALID, "sso_assertion_invalid");
        assert_eq!(SIGN_IN_REFUSED, "sso_sign_in_refused");
        assert_eq!(SIGN_IN_FAILED, "sso_sign_in_failed");
        assert_eq!(DOMAIN_REQUIRED, "sso_domain_required");
        assert_eq!(VERIFIED_EMAIL_UNSUPPORTED, "sso_verified_email_unsupported");
        assert_eq!(PROTOCOL_MISMATCH, "sso_protocol_mismatch");
    }

    #[test]
    fn sign_in_failed_is_no_error_code() {
        assert!(every().iter().all(|error| error.code() != SIGN_IN_FAILED));
    }

    #[test]
    fn the_required_message_is_one_plain_sentence_pair() {
        let message = Error::Required.to_string();
        assert_eq!(
            message,
            "This server requires single sign-on. Sign in from the Zone console in your browser; only server admins can sign in with a password."
        );
        assert!(!message.contains('\n') && !message.contains('<'));
    }

    #[test]
    fn every_message_is_one_line() {
        for error in every() {
            let message = error.to_string();
            assert!(!message.is_empty() && !message.contains('\n'), "{error:?}");
        }
    }

    #[test]
    fn domain_message_names_the_entry() {
        assert!(
            Error::DomainInvalid("*.acme.com".into())
                .to_string()
                .contains("*.acme.com")
        );
    }

    #[test]
    fn assertion_message_names_the_reason() {
        assert_eq!(
            Error::AssertionInvalid(Refusal::Audience).to_string(),
            "The identity provider's SAML response was refused because it's meant for a different service provider."
        );
    }

    #[test]
    fn sign_in_refused_message_names_the_status() {
        assert!(
            Error::SignInRefused("urn:oasis:names:tc:SAML:2.0:status:Requester".into())
                .to_string()
                .contains("status:Requester")
        );
    }

    #[test]
    fn id_token_message_names_the_reason() {
        assert_eq!(
            Error::IdTokenInvalid(Rejection::Nonce).to_string(),
            "The identity provider's ID token was refused because its nonce doesn't match this sign-in."
        );
    }
}
