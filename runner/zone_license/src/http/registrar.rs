use chrono::{DateTime, Utc};

use crate::gate::{Gate, Throttle};
use crate::registry::{RegistrySettings, TokenKey};

#[derive(Debug)]
pub struct Registrar {
    pub(crate) gate: Gate,
    pub(crate) key: TokenKey,
    pub(crate) settings: RegistrySettings,
    pub(crate) throttle: Throttle,
    pub(crate) clock: fn() -> DateTime<Utc>,
}

impl Registrar {
    pub fn new(gate: Gate, key: TokenKey, settings: RegistrySettings, throttle: Throttle) -> Self {
        Self {
            gate,
            key,
            settings,
            throttle,
            clock: Utc::now,
        }
    }

    pub fn with_clock(self, clock: fn() -> DateTime<Utc>) -> Self {
        Self {
            gate: self.gate.with_clock(clock),
            clock,
            ..self
        }
    }

    pub fn service(&self) -> &str {
        &self.settings.service
    }

    pub fn key_id(&self) -> &str {
        self.key.key_id()
    }
}
