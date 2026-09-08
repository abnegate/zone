//! GitHub App authentication.
//!
//! A personal access token carries one human's whole account into every
//! request a workspace makes. An App install carries only what the install was
//! granted, on its own rate limit, and survives that human leaving. This module
//! is the second half of that: the credential path, not the setup flow.
//!
//! Authenticating is two steps. A JWT signed with the App's private key proves
//! we are the App; GitHub trades that JWT for an access token scoped to one
//! installation, good for an hour. The JWT and the token are both bearer
//! credentials, so both are [`SecretValue`]s from the moment they exist —
//! neither can reach a log line, an error string or a `Debug` output.
//!
//! [`Provider`] is the entry point. It answers "what credential should this
//! source's requests carry", minting and caching installation tokens for
//! sources configured with an App and decrypting the stored personal access
//! token for those that are not. Which of the two a caller received is not
//! visible to it.
//!
//! [`SecretValue`]: zone_core::secret::SecretValue

pub mod cache;
pub mod claims;
pub mod configuration;
pub mod credential;
pub mod error;
pub mod identifier;
pub mod issuer;
pub mod jwt;
pub mod provider;
pub mod token;

#[cfg(test)]
mod testing;

pub use cache::TokenCache;
pub use claims::{CLOCK_SKEW, Claims, LIFETIME};
pub use configuration::{AppConfiguration, CONFIGURATION_KEY, StoredConfiguration};
pub use credential::Credential;
pub use error::GithubAppError;
pub use identifier::{ApplicationId, InstallationId};
pub use issuer::{GITHUB_API_ORIGIN, Issuer};
pub use provider::Provider;
pub use token::{InstallationToken, SAFETY_MARGIN};
