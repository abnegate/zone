use std::fmt;

use abnegate_secret::REDACTED;
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use reqwest::header::CONTENT_TYPE;
use serde::Serialize;
use zone_enterprise::audit::destination::{WebhookSecrets, WebhookSettings};
use zone_enterprise::audit::signature::{SIGNATURE_HEADER, TIMESTAMP_HEADER, sign};
use zone_enterprise::audit::{Event, FailureCode};

use super::{Batch, Failure, Transport, client};

pub const CONTENT: &str = "application/json";

#[derive(Serialize)]
struct Payload<'a> {
    events: &'a [Event],
}

pub struct Webhook {
    url: String,
    secret: String,
    client: Client,
}

impl Webhook {
    pub fn new(settings: &WebhookSettings, secrets: &WebhookSecrets, client: Client) -> Self {
        Self {
            url: settings.url.clone(),
            secret: secrets.signing_secret.clone(),
            client,
        }
    }
}

impl fmt::Debug for Webhook {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Webhook")
            .field("url", &self.url)
            .field("secret", &REDACTED)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Transport for Webhook {
    async fn deliver(&self, batch: &Batch) -> Result<(), Failure> {
        if batch.events.is_empty() {
            return Ok(());
        }
        let url = client::target(&self.url)?;
        let body = serde_json::to_vec(&Payload {
            events: &batch.events,
        })
        .map_err(|error| {
            tracing::error!(%error, "Could not serialize audit events for a webhook");
            Failure::of(FailureCode::Configuration)
        })?;
        let timestamp = Utc::now().timestamp();
        let signature = sign(self.secret.as_bytes(), timestamp, &body);
        let response = self
            .client
            .post(url)
            .header(CONTENT_TYPE, CONTENT)
            .header(TIMESTAMP_HEADER, timestamp.to_string())
            .header(SIGNATURE_HEADER, signature)
            .body(body)
            .send()
            .await
            .map_err(|error| client::failure(&error))?;
        client::accepted(&response)
    }
}
