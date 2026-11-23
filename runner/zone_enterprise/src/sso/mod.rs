mod account;
mod algorithm;
mod audience;
mod authorization;
mod body;
mod cached;
mod claims;
mod client_authentication;
mod client_id;
mod client_secret;
mod clock;
mod discovery;
mod display_name;
mod domains;
mod encoded_header;
mod error;
mod fields;
mod grant;
mod header;
mod identity;
mod issuer;
mod keys;
mod oidc;
mod password_access;
mod pkce;
mod policy;
mod protocol;
mod provider;
mod random;
mod rejection;
mod resolution;
pub mod saml;
#[cfg(any(test, feature = "signing"))]
pub mod testing;
mod token_refusal;
mod token_response;
mod unread;
mod userinfo;
mod verifier;

pub(crate) const REDACTED: &str = "<redacted>";

pub use account::{ACCOUNT_DISABLED, Account};
pub use audience::Audience;
pub use authorization::{Authorization, SCOPE};
pub use claims::Claims;
pub use client_authentication::ClientAuthentication;
pub use client_id::ClientId;
pub use client_secret::ClientSecret;
pub use clock::{Clock, system};
pub use discovery::Discovery;
pub use display_name::DisplayName;
pub use domains::Domains;
pub use error::{
    ASSERTION_INVALID, CLIENT_ID_INVALID, CLIENT_SECRET_INVALID, DISCOVERY_FAILED,
    DISPLAY_NAME_INVALID, DOMAIN_INVALID, DOMAIN_NOT_ALLOWED, DOMAIN_REQUIRED, EMAIL_CONFLICT,
    EMAIL_MISSING, EMAIL_UNVERIFIED, Error, ID_TOKEN_INVALID, ISSUER_INVALID, METADATA_INVALID,
    PROTOCOL_MISMATCH, PROVIDER_DUPLICATE, PROVIDER_NOT_FOUND, PROVIDER_REQUIRED, RATE_LIMITED,
    SIGN_IN_FAILED, SIGN_IN_REFUSED, SSO_REQUIRED, STATE_INVALID, TOKEN_EXCHANGE_FAILED,
    VERIFIED_EMAIL_UNSUPPORTED,
};
pub use fields::Fields;
pub use grant::Grant;
pub use identity::Identity;
pub use issuer::{GOOGLE, GOOGLE_HOST, Issuer, endpoint_allowed};
pub use oidc::{Oidc, REFETCH_COOLDOWN, TIMEOUT};
pub use password_access::{PasswordAccess, password_access};
pub use pkce::{METHOD, Pkce, challenge};
pub use policy::Policy;
pub use protocol::Protocol;
pub use provider::Provider;
pub use rejection::Rejection;
pub use resolution::{Resolution, resolve};
