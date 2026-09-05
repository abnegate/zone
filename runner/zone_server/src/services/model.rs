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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelProfile {
    pub completion: Option<bool>,
    pub tools: Option<bool>,
    pub needs_character: bool,
}

#[derive(Deserialize, Default)]
pub struct Model {
    capabilities: Option<Vec<String>>,
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    system: Option<String>,
    #[serde(default)]
    modelfile: Option<String>,
}

impl Model {
    pub async fn completion(host: &str, name: &str) -> Option<bool> {
        Self::profile(host, name).await.completion
    }

    pub async fn profile(host: &str, name: &str) -> ModelProfile {
        match Self::show(host, name).await {
            Some(model) => model.into_profile(name),
            None => ModelProfile {
                completion: None,
                tools: None,
                needs_character: needs_character(name, None, false),
            },
        }
    }

    async fn show(host: &str, name: &str) -> Option<Self> {
        let response = CLIENT
            .post(format!("{}/api/show", host.trim_end_matches('/')))
            .json(&serde_json::json!({"model": name}))
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        response.json().await.ok()
    }

    fn into_profile(&self, name: &str) -> ModelProfile {
        let tools = self.supports_tools();
        ModelProfile {
            completion: self.supports_completion(),
            tools,
            needs_character: needs_character(name, tools, self.expects_persona()),
        }
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

    fn supports_tools(&self) -> Option<bool> {
        let capabilities = self.capabilities.as_ref()?;
        Some(capabilities.iter().any(|capability| capability == "tools"))
    }

    fn expects_persona(&self) -> bool {
        [&self.template, &self.system, &self.modelfile]
            .into_iter()
            .flatten()
            .any(|text| has_persona_placeholder(text))
    }
}

/// A card-shaped template interpolates speaker slots instead of a single assistant.
fn has_persona_placeholder(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("{{char}}") || lower.contains("{{user}}")
}

fn needs_character(name: &str, tools: Option<bool>, expects_persona: bool) -> bool {
    if expects_persona {
        return true;
    }
    if tools == Some(true) || looks_assistant(name) {
        return false;
    }
    is_imported_weights(name)
}

fn looks_assistant(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("instruct") || name_tokens(&lower).any(|token| matches!(token, "chat" | "it"))
}

fn is_imported_weights(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("hf.co/")
}

fn name_tokens(name: &str) -> impl Iterator<Item = &str> {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{Model, ModelProfile, needs_character};
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    fn model(capabilities: Option<Vec<&str>>) -> Model {
        match capabilities {
            Some(values) => serde_json::from_value(json!({ "capabilities": values })).unwrap(),
            None => serde_json::from_value(json!({})).unwrap(),
        }
    }

    fn model_with(body: serde_json::Value) -> Model {
        serde_json::from_value(body).unwrap()
    }

    #[test]
    fn tools_follow_engine_capabilities() {
        assert_eq!(
            model(Some(vec!["completion", "tools"])).supports_tools(),
            Some(true)
        );
        assert_eq!(
            model(Some(vec!["completion"])).supports_tools(),
            Some(false)
        );
        assert_eq!(model(Some(vec![])).supports_tools(), Some(false));
        assert_eq!(model(None).supports_tools(), None);
    }

    #[test]
    fn character_is_for_imported_weights_and_persona_templates() {
        assert!(needs_character("hf.co/owner/custom-7b-Q4_K_M", None, false));
        assert!(needs_character(
            "llama3.1:latest",
            None,
            true
        ));
        assert!(!needs_character("llama3.1:latest", None, false));
        assert!(!needs_character("mistral", None, false));
        assert!(!needs_character(
            "hf.co/owner/Llama-3.1-8B-Instruct-GGUF:Q4_K_M",
            None,
            false
        ));
        assert!(!needs_character("hf.co/owner/open-chat-7b", None, false));
        assert!(!needs_character("gemma2:it", None, false));
        assert!(!needs_character("hf.co/owner/custom-7b", Some(true), false));
    }

    #[test]
    fn profile_reads_persona_placeholders_from_the_engine() {
        let tool_model = model(Some(vec!["completion", "tools"]));
        assert_eq!(
            tool_model.into_profile("llama3.1:latest"),
            ModelProfile {
                completion: Some(true),
                tools: Some(true),
                needs_character: false,
            }
        );
        let imported = model(Some(vec!["completion"]));
        assert_eq!(
            imported.into_profile("hf.co/owner/custom-7b:latest"),
            ModelProfile {
                completion: Some(true),
                tools: Some(false),
                needs_character: true,
            }
        );
        let persona = model_with(json!({
            "capabilities": ["completion"],
            "template": "{{char}}: {{.Prompt}}"
        }));
        assert_eq!(
            persona.into_profile("custom:latest"),
            ModelProfile {
                completion: Some(true),
                tools: Some(false),
                needs_character: true,
            }
        );
    }

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

    #[tokio::test]
    async fn profile_survives_a_missing_engine_using_the_model_name() {
        let server = MockServer::start().await;
        let name = "hf.co/owner/custom-7b";
        Mock::given(method("POST"))
            .and(path("/api/show"))
            .and(body_json(json!({"model": name})))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        assert_eq!(
            Model::profile(&server.uri(), name).await,
            ModelProfile {
                completion: None,
                tools: None,
                needs_character: true,
            }
        );
    }
}
