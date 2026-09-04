//! Capabilities reported by the installed inference engine.

use reqwest::Client;
use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .connect_timeout(Duration::from_secs(2))
        // Metadata is advisory: do not delay inference on an unavailable engine.
        .timeout(Duration::from_secs(2))
        .build()
        .expect("Failed to build model metadata client")
});

pub const UNSUPPORTED: &str =
    "This model supports embeddings, not chat responses. Choose a model that supports chat.";

#[derive(Deserialize)]
pub struct Model {
    capabilities: Option<Vec<String>>,
}

impl Model {
    pub async fn completion(host: &str, name: &str) -> Option<bool> {
        let response = CLIENT
            .post(format!("{}/api/show", host.trim_end_matches('/')))
            .json(&serde_json::json!({"model": name}))
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let model: Self = response.json().await.ok()?;
        model.supports_completion()
    }

    fn supports_completion(&self) -> Option<bool> {
        let capabilities = self.capabilities.as_ref()?;
        if capabilities
            .iter()
            .any(|capability| capability == "completion")
        {
            Some(true)
        } else if capabilities
            .iter()
            .any(|capability| capability == "embedding")
        {
            Some(false)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Model;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    #[tokio::test]
    async fn malformed_or_slow_metadata_remains_unknown() {
        let server = MockServer::start().await;
        for (name, response) in [
            (
                "malformed",
                ResponseTemplate::new(200).set_body_string("not json"),
            ),
            (
                "wrong-type",
                ResponseTemplate::new(200).set_body_json(json!({"capabilities": "completion"})),
            ),
            (
                "slow",
                ResponseTemplate::new(200)
                    .set_body_json(json!({"capabilities": ["embedding"]}))
                    .set_delay(std::time::Duration::from_secs(3)),
            ),
        ] {
            Mock::given(method("POST"))
                .and(path("/api/show"))
                .and(body_json(json!({"model": name})))
                .respond_with(response)
                .mount(&server)
                .await;
            assert_eq!(Model::completion(&server.uri(), name).await, None, "{name}");
        }
    }

    #[tokio::test]
    async fn completion_uses_authoritative_metadata_and_preserves_unknown_models() {
        let server = MockServer::start().await;
        for (name, response, expected) in [
            (
                "hf.co/owner/custom:Q4",
                json!({"capabilities": ["completion", "tools"]}),
                Some(true),
            ),
            (
                "vector:latest",
                json!({"capabilities": ["embedding"]}),
                Some(false),
            ),
            (
                "mixed",
                json!({"capabilities": ["embedding", "completion"]}),
                Some(true),
            ),
            ("old", json!({}), None),
            ("empty", json!({"capabilities": []}), None),
            ("future", json!({"capabilities": ["unknown"]}), None),
        ] {
            Mock::given(method("POST"))
                .and(path("/api/show"))
                .and(body_json(json!({"model": name})))
                .respond_with(ResponseTemplate::new(200).set_body_json(response))
                .expect(1)
                .mount(&server)
                .await;
            assert_eq!(
                Model::completion(&server.uri(), name).await,
                expected,
                "{name}"
            );
        }
        for status in [404, 500] {
            let name = format!("unavailable-{status}");
            Mock::given(method("POST"))
                .and(path("/api/show"))
                .and(body_json(json!({"model": name})))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;
            assert_eq!(Model::completion(&server.uri(), &name).await, None);
        }
    }
}
