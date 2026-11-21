use serde::Serialize;
use zone_enterprise::License;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Automatic,
    Disabled,
    Manual,
}

impl Mode {
    pub fn of(license: &License, enabled: bool) -> Self {
        match (license.subscription(), enabled) {
            (None, _) => Self::Manual,
            (Some(_), true) => Self::Automatic,
            (Some(_), false) => Self::Disabled,
        }
    }
}

#[cfg(test)]
mod tests;
