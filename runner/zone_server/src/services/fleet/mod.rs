//! Runner fleet policy: how this server process registers as a runner and claims work.

pub mod claimant;
pub mod flight;
pub mod identity;
pub mod misconfigured;
pub mod refusal;
pub mod registration;
pub mod retention;
#[cfg(test)]
pub(crate) mod tests;

pub use claimant::claimant;
pub use flight::Flight;
pub use identity::Identity;
pub use misconfigured::Misconfigured;
