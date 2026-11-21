use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    Url,
    Credentials,
    Metadata,
    Plaintext,
}

impl SettingsError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Url => "ZONE_LICENSE_RENEWAL_URL must be an http or https URL",
            Self::Credentials => "ZONE_LICENSE_RENEWAL_URL must not carry a user name or password",
            Self::Metadata => "ZONE_LICENSE_RENEWAL_URL must not name a cloud metadata address",
            Self::Plaintext => {
                "ZONE_LICENSE_RENEWAL_URL must use https unless it names a loopback or private network host"
            }
        }
    }
}

impl From<abnegate_http::Error> for SettingsError {
    fn from(error: abnegate_http::Error) -> Self {
        match error {
            abnegate_http::Error::MetadataTarget => Self::Metadata,
            abnegate_http::Error::EmbeddedCredentials => Self::Credentials,
            _ => Self::Url,
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for SettingsError {}
