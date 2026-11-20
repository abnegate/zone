use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    Url,
    Metadata,
}

impl SettingsError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Url => "ZONE_LICENSE_RENEWAL_URL must be an http or https URL",
            Self::Metadata => "ZONE_LICENSE_RENEWAL_URL must not name a cloud metadata address",
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for SettingsError {}
