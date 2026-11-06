//! Credentials sealed at rest under the deployment's master key.

mod derivation;
mod unseal;

pub use derivation::derive;
pub use unseal::Unseal;
