#[cfg(feature = "audit")]
pub mod audit;
mod duplicate_key;
mod edition;
mod encoding;
mod entitlement;
mod error;
#[cfg(test)]
mod fixtures;
pub mod fleet;
mod header;
mod key_error;
mod keyring;
mod license;
mod members;
mod payload;
mod plan;
pub mod policy;
mod production;
mod seats;
#[cfg(any(test, feature = "signing"))]
mod signing;
pub mod spend;
#[cfg(feature = "sso")]
pub mod sso;
mod status;
mod trusted_key;
mod unknown_plan;

pub use duplicate_key::DuplicateKey;
pub use edition::Edition;
pub use encoding::{SEPARATOR, encode};
pub use entitlement::{Feature, LICENSE_REQUIRED};
pub use error::{Error, MALFORMED, PAYLOAD_INVALID, SIGNATURE_INVALID, UNKNOWN_KEY};
pub use key_error::KeyError;
pub use keyring::Keyring;
pub use license::License;
pub use members::{COMMUNITY_MEMBERS, MEMBER_CAP_REACHED, Members};
pub use payload::Payload;
pub use plan::Plan;
pub use seats::{Seats, period_start};
#[cfg(any(test, feature = "signing"))]
pub use signing::{SigningKey, sign, sign_bytes};
pub use status::{GRACE_DAYS, Status, status};
pub use trusted_key::TrustedKey;
pub use unknown_plan::UnknownPlan;
