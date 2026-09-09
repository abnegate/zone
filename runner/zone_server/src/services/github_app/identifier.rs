//! The two numeric identifiers GitHub App authentication turns on.
//!
//! Both are bare integers on the wire and both live side by side in the same
//! configuration, so swapping them yields a `401` at runtime rather than an
//! error at compile time. Each gets its own type instead.

use serde::{Deserialize, Serialize};

/// The App's own identifier, the `iss` of every JWT it signs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApplicationId(u64);

impl ApplicationId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One installation of the App on an account or organisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstallationId(u64);

impl InstallationId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_bare_integers_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&ApplicationId::new(12345)).expect("serialise"),
            "12345"
        );
        assert_eq!(
            serde_json::from_str::<InstallationId>("67890").expect("deserialise"),
            InstallationId::new(67890)
        );
    }
}
