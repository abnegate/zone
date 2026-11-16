//! Server-wide single sign-on: provider configuration and enforcement. The protocol, validation
//! and enforcement policy live in zone_enterprise; this wires them to storage and the audit log.

pub mod enforcement;
pub mod failure;
pub mod providers;
pub mod revision;

pub use failure::Failure;
pub use revision::Revision;
