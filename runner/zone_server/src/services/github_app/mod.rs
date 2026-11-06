//! GitHub App authentication.
//!
//! A personal access token carries one human's whole account into every
//! request a workspace makes. An App installation carries only what it was
//! granted, on its own rate limit, and survives that human leaving.
//!
//! Authenticating is two steps, both done by `abnegate_vcs`: a JWT signed with
//! the App's private key proves Zone is the App, and GitHub trades that JWT
//! for an access token scoped to one installation, good for an hour. Both are
//! bearer credentials, so both are [`SecretValue`]s from the moment they
//! exist, and no [`Error`] carries either, the private key, or a URL.
//!
//! [`Provider`] is the entry point. It seals and opens App private keys at
//! rest, mints and caches installation tokens, and opens the personal access
//! token stored against a source or project. Every path hands back a
//! [`Credential`], so a caller cannot tell which origin it received.
//!
//! [`SecretValue`]: abnegate_secret::SecretValue

pub mod credential;
pub mod error;
pub mod provider;

#[cfg(test)]
pub(crate) mod testing;

pub use credential::Credential;
pub use error::Error;
pub use provider::Provider;
