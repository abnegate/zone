//! Server-wide single sign-on: provider configuration, enforcement and signing in. The protocol,
//! validation and policy live in zone_enterprise; this wires them to storage, sessions and the
//! audit log.

pub mod callback;
pub mod enforcement;
pub mod failure;
pub mod providers;
pub mod revision;
pub mod settlement;
pub mod sign_in;
pub mod sign_in_failure;

pub use callback::Callback;
pub use failure::Failure;
pub use revision::Revision;
pub use settlement::Settlement;
pub use sign_in_failure::SignInFailure;
