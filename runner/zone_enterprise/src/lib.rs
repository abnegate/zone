mod edition;
mod encoding;
mod error;
#[cfg(test)]
mod fixtures;
mod header;
mod keyring;
mod license;
mod payload;
mod production;
mod seats;
#[cfg(any(test, feature = "signing"))]
mod signing;
mod status;
mod tier;
mod trusted_key;

pub use edition::Edition;
pub use encoding::{SEPARATOR, encode};
pub use error::{Error, KEY_INVALID, MALFORMED, PAYLOAD_INVALID, SIGNATURE_INVALID, UNKNOWN_KEY};
pub use keyring::Keyring;
pub use license::License;
pub use payload::Payload;
pub use seats::{Seats, period_start};
#[cfg(any(test, feature = "signing"))]
pub use signing::{SigningKey, sign, sign_bytes};
pub use status::{GRACE_DAYS, Status, status};
pub use tier::Tier;
pub use trusted_key::TrustedKey;
