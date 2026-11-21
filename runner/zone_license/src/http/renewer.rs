use chrono::{DateTime, Utc};
use zone_enterprise::Keyring;

use crate::polar::Client;

#[derive(Debug, Clone)]
pub struct Renewer {
    pub(crate) keyring: Keyring,
    pub(crate) client: Client,
    pub(crate) clock: fn() -> DateTime<Utc>,
}

impl Renewer {
    pub fn new(keyring: Keyring, client: Client) -> Self {
        Self {
            keyring,
            client,
            clock: Utc::now,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_clock(self, clock: fn() -> DateTime<Utc>) -> Self {
        Self { clock, ..self }
    }
}
