use std::fmt;
use std::sync::Arc;

use abnegate_secret::{MasterKey, REDACTED};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::audit::FailureCode;
use zone_enterprise::audit::destination::{Secrets, Settings};

use super::s3::S3;
use super::syslog::Syslog;
use super::tls::Configurations;
use super::webhook::Webhook;
use super::{Failure, Transport, client};
use crate::db::audit::destinations::DestinationRecord;
use crate::services::audit::delivery::Open;
use crate::services::audit::destinations;
use crate::services::hosts::Hosts;

pub const MISMATCHED: &str = "The destination's settings and secrets are for different kinds";

/// Opens each destination with the real transport for its kind, sharing the process's one HTTP
/// client and each destination's TLS configuration until the destination changes.
pub struct Transports {
    key: Arc<MasterKey>,
    hosts: Hosts,
    configurations: Configurations,
}

impl Transports {
    pub fn new(key: Arc<MasterKey>, hosts: Hosts) -> Self {
        Self {
            key,
            hosts,
            configurations: Configurations::default(),
        }
    }

    fn transport(
        &self,
        id: Uuid,
        updated_at: DateTime<Utc>,
        settings: &Settings,
        secrets: &Secrets,
    ) -> Result<Box<dyn Transport>, Failure> {
        match (settings, secrets) {
            (Settings::Webhook(settings), Secrets::Webhook(secrets)) => {
                Ok(Box::new(Webhook::new(settings, secrets, client::client()?)))
            }
            (Settings::S3(settings), Secrets::S3(secrets)) => {
                Ok(Box::new(S3::new(settings, secrets, client::client()?)))
            }
            (Settings::Syslog(settings), Secrets::Syslog(secrets)) => Ok(Box::new(
                Syslog::reusing(settings, secrets, &self.configurations, id, updated_at)?,
            )),
            _ => Err(Failure::new(FailureCode::Configuration, MISMATCHED)),
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
        self.transport(destination.id, destination.updated_at, &settings, &secrets)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zone_enterprise::audit::Kind;
    use zone_enterprise::audit::destination::WebhookSettings;

    use super::*;

    fn transports() -> Transports {
        Transports::new(
            Arc::new(MasterKey::generate().expect("a key")),
            Hosts::default(),
        )
    }

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
        let failure = transports()
            .transport(Uuid::new_v4(), Utc::now(), &settings, &secrets)
            .err()
            .expect("mismatched kinds are refused");
        assert_eq!(failure.code, FailureCode::Configuration);
        assert_eq!(failure.message, MISMATCHED);
    }

    #[test]
    fn a_tls_syslog_destination_reuses_its_configuration_until_it_changes() {
        let transports = transports();
        let settings = Settings::Syslog(
            serde_json::from_value(json!({"host": "siem.lan", "port": 6514, "protocol": "tls"}))
                .unwrap(),
        );
        let secrets = Secrets::parse(Kind::Syslog, json!({})).unwrap();
        let id = Uuid::new_v4();
        let saved = Utc::now();
        for _ in 0..3 {
            transports
                .transport(id, saved, &settings, &secrets)
                .expect("a TLS syslog transport");
        }
        assert_eq!(transports.configurations.len(), 1);
        transports
            .transport(Uuid::new_v4(), saved, &settings, &secrets)
            .expect("a second destination");
        assert_eq!(transports.configurations.len(), 2);
    }

    #[test]
    fn debug_never_shows_the_key() {
        let shown = format!("{:?}", transports());
        assert!(shown.contains(REDACTED), "{shown}");
    }
}
