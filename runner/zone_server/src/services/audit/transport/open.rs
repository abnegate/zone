use std::fmt;
use std::sync::Arc;

use abnegate_secret::{MasterKey, REDACTED};
use async_trait::async_trait;
use reqwest::Client;
use zone_enterprise::audit::FailureCode;
use zone_enterprise::audit::destination::{Secrets, Settings};

use super::s3::S3;
use super::syslog::Syslog;
use super::webhook::Webhook;
use super::{Failure, Transport, client};
use crate::db::audit::destinations::DestinationRecord;
use crate::services::audit::delivery::Open;
use crate::services::audit::destinations;
use crate::services::hosts::Hosts;

pub const MISMATCHED: &str = "The destination's settings and secrets are for different kinds";

/// Opens each destination with the real transport for its kind, sharing one HTTP client.
pub struct Transports {
    client: Client,
    key: Arc<MasterKey>,
    hosts: Hosts,
}

impl Transports {
    pub fn new(key: Arc<MasterKey>, hosts: Hosts) -> Self {
        Self {
            client: client::client(),
            key,
            hosts,
        }
    }
}

impl fmt::Debug for Transports {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Transports")
            .field("key", &REDACTED)
            .field("hosts", &self.hosts)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Open for Transports {
    async fn open(&self, destination: &DestinationRecord) -> Result<Box<dyn Transport>, Failure> {
        let (settings, secrets) = destinations::open(destination, &self.key, &self.hosts)?;
        transport(&settings, &secrets, self.client.clone())
    }
}

pub fn transport(
    settings: &Settings,
    secrets: &Secrets,
    client: Client,
) -> Result<Box<dyn Transport>, Failure> {
    match (settings, secrets) {
        (Settings::Webhook(settings), Secrets::Webhook(secrets)) => {
            Ok(Box::new(Webhook::new(settings, secrets, client)))
        }
        (Settings::S3(settings), Secrets::S3(secrets)) => {
            Ok(Box::new(S3::new(settings, secrets, client)))
        }
        (Settings::Syslog(settings), Secrets::Syslog(secrets)) => {
            Ok(Box::new(Syslog::new(settings, secrets)?))
        }
        _ => Err(Failure::new(FailureCode::Configuration, MISMATCHED)),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zone_enterprise::audit::Kind;
    use zone_enterprise::audit::destination::WebhookSettings;

    use super::*;

    #[test]
    fn settings_and_secrets_of_different_kinds_are_a_configuration_failure() {
        let settings = Settings::Webhook(WebhookSettings {
            url: "https://siem.example.com/hook".into(),
        });
        let secrets = Secrets::parse(
            Kind::S3,
            json!({"access_key_id": "a", "secret_access_key": "b"}),
        )
        .unwrap();
        let failure = transport(&settings, &secrets, client::client())
            .err()
            .expect("mismatched kinds are refused");
        assert_eq!(failure.code, FailureCode::Configuration);
        assert_eq!(failure.message, MISMATCHED);
    }

    #[test]
    fn debug_never_shows_the_key() {
        let key = Arc::new(MasterKey::generate().expect("a key"));
        let shown = format!("{:?}", Transports::new(key, Hosts::default()));
        assert!(shown.contains(REDACTED), "{shown}");
    }
}
