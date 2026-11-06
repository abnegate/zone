//! Credentials sealed at rest under the deployment's master key.

mod derivation;
#[cfg(test)]
pub(crate) mod fixture;
mod unseal;

pub use derivation::derive;
pub use unseal::Unseal;
