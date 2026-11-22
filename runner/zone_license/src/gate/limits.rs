use std::num::NonZeroU32;
use std::time::Duration;

pub const WINDOW: Duration = Duration::from_secs(60);
pub const DEFAULT_ADDRESS_LIMIT: NonZeroU32 = NonZeroU32::new(120).unwrap();
pub const DEFAULT_LICENSE_LIMIT: NonZeroU32 = NonZeroU32::new(240).unwrap();

/// Requests allowed per `WINDOW`, per client address and per license ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub address: NonZeroU32,
    pub license: NonZeroU32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            address: DEFAULT_ADDRESS_LIMIT,
            license: DEFAULT_LICENSE_LIMIT,
        }
    }
}
