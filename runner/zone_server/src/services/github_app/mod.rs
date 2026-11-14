//! GitHub App authentication. No [`Error`] carries a JWT, an installation
//! token, a private key, or a URL.

pub mod access;
pub mod address;
pub mod catalog;
pub mod credential;
pub mod error;
pub mod failure;
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

use std::num::NonZeroU8;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use credential::Credential;
pub use error::Error;
pub use failure::Failure;
pub use provider::Provider;
pub use unauthenticated::Unauthenticated;
pub use unrecorded::Unrecorded;

pub const PAGE_SIZE: NonZeroU8 =
    NonZeroU8::new(100).expect("a GitHub page holds at least one item");
