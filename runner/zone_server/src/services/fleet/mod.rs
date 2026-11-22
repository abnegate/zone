//! Runner fleet policy: how this server process registers as a runner and claims work.

pub mod claimant;
pub mod identity;
pub mod refusal;
pub mod registration;
#[cfg(test)]
pub(crate) mod tests;

pub use claimant::claimant;
pub use identity::Identity;
