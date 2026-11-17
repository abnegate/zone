use std::fmt;

use serde::{Deserialize, Serialize};

use crate::audit::destination::{REDACTED, url};
use crate::audit::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookSettings {
    pub url: String,
}

impl WebhookSettings {
    pub fn validated(self) -> Result<Self, Error> {
        Ok(Self {
            url: url(&self.url)?,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookSecrets {
    pub signing_secret: String,
}

impl fmt::Debug for WebhookSecrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebhookSecrets")
            .field("signing_secret", &REDACTED)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::url_reason::UrlReason;

    fn validated(url: &str) -> Result<WebhookSettings, Error> {
        WebhookSettings { url: url.into() }.validated()
    }

    #[test]
    fn plain_http_to_a_lan_host_is_accepted() {
        for url in [
            "http://10.0.0.5/hook",
            "http://192.168.1.20:8088/services/collector",
            "http://splunk:8088/services/collector",
            "http://127.0.0.1:9000",
        ] {
            assert_eq!(validated(url).unwrap().url, url);
        }
    }

    #[test]
    fn the_url_is_trimmed() {
        assert_eq!(
            validated("  https://hooks.example.com/zone \n")
                .unwrap()
                .url,
            "https://hooks.example.com/zone"
        );
    }

    #[test]
    fn network_location_is_left_to_the_server() {
        assert!(validated("http://169.254.169.254/latest").is_ok());
    }

    #[test]
    fn a_url_with_credentials_is_refused() {
        assert_eq!(
            validated("https://zone:secret@hooks.example.com"),
            Err(Error::UrlInvalid(UrlReason::Userinfo))
        );
    }
}
