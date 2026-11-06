//! GitHub App authentication. No [`Error`] carries a JWT, an installation
//! token, a private key, or a URL.

pub mod access;
pub mod credential;
pub mod error;
pub mod login;
pub mod manifest;
pub mod permissions;
pub mod provider;
mod scope;
pub mod unauthenticated;

#[cfg(test)]
pub(crate) mod testing;

pub use credential::Credential;
pub use error::Error;
pub use provider::Provider;
pub use unauthenticated::Unauthenticated;
