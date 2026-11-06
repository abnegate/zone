//! GitHub App authentication. No [`Error`] carries a JWT, an installation
//! token, a private key, or a URL.

pub mod access;
pub mod catalog;
pub mod credential;
pub mod error;
mod flight;
mod flights;
pub mod installations;
pub mod login;
pub mod manifest;
mod mint;
pub mod permissions;
pub mod provider;
mod scope;
mod tenant;
pub mod unauthenticated;
pub mod unrecorded;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use credential::Credential;
pub use error::Error;
pub use provider::Provider;
pub use unauthenticated::Unauthenticated;
pub use unrecorded::Unrecorded;
