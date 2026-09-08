//! Credential handling
//!
//! [`SecretValue`] holds a credential without letting it reach a log line by
//! accident, [`encryption`] wraps one in an `ENC[v1:...]` envelope for storage,
//! and [`redact`] scrubs credentials out of text on its way back to a model.

pub mod encryption;

mod redact;
mod value;

#[cfg(feature = "sqlx")]
mod database;

pub use encryption::{MasterKey, SecretError};
pub use redact::{REDACTED, redact};
pub use value::{OptionalSecretExt, SecretValue};
