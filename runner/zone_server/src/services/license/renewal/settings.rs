use std::fmt;

use abnegate_http::TargetPolicy;
use reqwest::Url;

use super::SettingsError;
use super::private::private;
use crate::config::Environment;

pub const ENABLED_VARIABLE: &str = "ZONE_LICENSE_RENEWAL";
pub const URL_VARIABLE: &str = "ZONE_LICENSE_RENEWAL_URL";
pub const DEFAULT_URL: &str = "https://licenses.bitecode.ltd/licenses/renew";

const PLAINTEXT_SCHEME: &str = "http";

#[derive(Clone, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub url: Url,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            url: validated(DEFAULT_URL).expect("the default renewal URL is valid"),
        }
    }
}

impl fmt::Debug for Settings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Settings")
            .field("enabled", &self.enabled)
            .field("url", &self.url.as_str())
            .finish()
    }
}

impl Settings {
    pub fn from_process_environment() -> Result<Self, SettingsError> {
        Self::from_lookup(|variable| std::env::var(variable).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, SettingsError> {
        let environment = Environment::new(lookup);
        let defaults = Self::default();
        let url = match environment.read(URL_VARIABLE) {
            Some(url) => validated(&url)?,
            None => defaults.url,
        };
        Ok(Self {
            enabled: environment
                .flag(ENABLED_VARIABLE)
                .unwrap_or(defaults.enabled),
            url,
        })
    }
}

pub fn validated(url: &str) -> Result<Url, SettingsError> {
    let url = TargetPolicy::ExceptMetadata
        .validate(url.trim())
        .map_err(|error| {
            tracing::warn!(variable = URL_VARIABLE, %error, "Refused the license renewal URL");
            SettingsError::from(error)
        })?;
    if url.scheme() == PLAINTEXT_SCHEME && !private(&url) {
        tracing::warn!(
            variable = URL_VARIABLE,
            host = url.host_str(),
            "Refused a plain http license renewal URL to a public host"
        );
        return Err(SettingsError::Plaintext);
    }
    Ok(url)
}

#[cfg(test)]
mod tests;
