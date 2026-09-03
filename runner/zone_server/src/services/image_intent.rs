//! Hybrid image-generation intent classification.

use serde_json::Value;
use std::time::Duration;
use zone_core::llm::{LlmClient, LlmConfig, Message};

use crate::config::ComfyUiConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleDecision {
    Image,
    Chat,
    Ambiguous,
}

#[derive(Clone)]
pub struct ImageIntentClassifier {
    config: ComfyUiConfig,
    litellm_host: String,
    litellm_key: String,
}

impl ImageIntentClassifier {
    pub fn new(config: ComfyUiConfig, litellm_host: String, litellm_key: String) -> Self {
        Self {
            config,
            litellm_host,
            litellm_key,
        }
    }

    /// Classify a message. Any unavailable, timed-out, or malformed model result
    /// safely falls back to normal chat.
    pub async fn is_image_request(&self, content: &str, metadata: Option<&Value>) -> bool {
        if let Some(force) = metadata
            .and_then(|m| m.get("image_generation"))
            .and_then(Value::as_bool)
        {
            return force;
        }
        if !self.config.enabled {
            return false;
        }

        match deterministic_decision(content) {
            RuleDecision::Image => true,
            RuleDecision::Chat => false,
            RuleDecision::Ambiguous => self.classify_ambiguous(content).await,
        }
    }

    async fn classify_ambiguous(&self, content: &str) -> bool {
        let client = LlmClient::new(LlmConfig {
            base_url: self.litellm_host.clone(),
            api_key: self.litellm_key.clone(),
            default_model: self.config.classifier_model.clone(),
            temperature: 0.0,
            max_tokens: 3,
        });
        let prompt = format!(
            "Return exactly IMAGE or CHAT. IMAGE only when the user wants a new image generated now. \
             Discussion, analysis, prompt-writing, coding, and editing instructions without asking to \
             generate now are CHAT.\nUser: {content}"
        );
        let result = tokio::time::timeout(
            Duration::from_secs(self.config.classifier_timeout_secs),
            client.chat_with_model(
                &self.config.classifier_model,
                vec![Message::user(prompt)],
                None,
            ),
        )
        .await;

        let Ok(Ok(response)) = result else {
            return false;
        };
        let Some(answer) = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
        else {
            return false;
        };
        answer.trim().eq_ignore_ascii_case("IMAGE")
    }
}

fn deterministic_decision(content: &str) -> RuleDecision {
    let text = content.trim().to_ascii_lowercase();
    if text.is_empty() {
        return RuleDecision::Chat;
    }

    // Negative guards take precedence over positive keywords.
    const NEGATIVE: &[&str] = &[
        "describe this image",
        "analyze this image",
        "analyse this image",
        "what is in this image",
        "edit this image",
        "modify this image",
        "image prompt",
        "prompt for an image",
        "improve this prompt",
        "how do i generate",
        "how to generate",
        "image generation code",
        "implement image",
        "comfyui workflow",
        "image api",
        "talk about",
        "explain",
    ];
    if NEGATIVE.iter().any(|guard| text.contains(guard)) {
        return RuleDecision::Chat;
    }

    const EXPLICIT: &[&str] = &[
        "generate an image",
        "generate a picture",
        "create an image",
        "create a picture",
        "make an image",
        "make a picture",
        "draw an image",
        "draw a picture",
        "render an image",
        "paint a picture",
        "illustrate ",
    ];
    if EXPLICIT.iter().any(|phrase| text.contains(phrase)) {
        return RuleDecision::Image;
    }

    const AMBIGUOUS: &[&str] = &[
        "draw ",
        "paint ",
        "render ",
        "sketch ",
        "visualize ",
        "visualise ",
        "poster",
        "logo",
        "wallpaper",
        "portrait",
        "illustration",
    ];
    if AMBIGUOUS.iter().any(|word| text.contains(word)) {
        RuleDecision::Ambiguous
    } else {
        RuleDecision::Chat
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    #[test]
    fn classifier_rule_matrix() {
        for request in [
            "Generate an image of a red panda",
            "please draw a picture of the moon",
            "Create a picture of our city",
            "Illustrate a quiet forest",
        ] {
            assert_eq!(
                deterministic_decision(request),
                RuleDecision::Image,
                "{request}"
            );
        }
        for chat in [
            "Explain image generation code",
            "Write an image prompt for a red panda",
            "Describe this image",
            "How do I generate an image with Rust?",
            "What is the capital of France?",
        ] {
            assert_eq!(deterministic_decision(chat), RuleDecision::Chat, "{chat}");
        }
        assert_eq!(
            deterministic_decision("Could you design a logo for Acme?"),
            RuleDecision::Ambiguous
        );
    }

    #[tokio::test]
    async fn metadata_override_and_disabled_guard() {
        let mut config = ComfyUiConfig {
            enabled: true,
            ..Default::default()
        };
        let classifier = ImageIntentClassifier::new(config.clone(), String::new(), String::new());
        assert!(
            classifier
                .is_image_request(
                    "hello",
                    Some(&serde_json::json!({"image_generation": true}))
                )
                .await
        );
        assert!(
            !classifier
                .is_image_request(
                    "generate an image",
                    Some(&serde_json::json!({"image_generation": false}))
                )
                .await
        );

        config.enabled = false;
        let disabled = ImageIntentClassifier::new(config, String::new(), String::new());
        assert!(
            disabled
                .is_image_request(
                    "generate an image",
                    Some(&serde_json::json!({"image_generation": true}))
                )
                .await
        );
    }

    #[tokio::test]
    async fn ambiguous_uses_strict_litellm_binary_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "classification",
                "object": "chat.completion",
                "created": 0,
                "model": "fast",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "IMAGE"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                classifier_model: "fast".to_string(),
                ..Default::default()
            },
            server.uri(),
            "key".to_string(),
        );
        assert!(
            classifier
                .is_image_request("Design a logo for Acme", None)
                .await
        );
    }

    #[tokio::test]
    async fn malformed_classifier_response_falls_back_to_chat() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "classification",
                "object": "chat.completion",
                "created": 0,
                "model": "fast",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "perhaps image"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                ..Default::default()
            },
            server.uri(),
            "key".to_string(),
        );
        assert!(
            !classifier
                .is_image_request("Design a logo for Acme", None)
                .await
        );
    }

    #[tokio::test]
    async fn classifier_timeout_falls_back_to_chat() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(2))
                    .set_body_json(serde_json::json!({})),
            )
            .mount(&server)
            .await;
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                classifier_timeout_secs: 1,
                ..Default::default()
            },
            server.uri(),
            "key".to_string(),
        );
        assert!(
            !classifier
                .is_image_request("Design a logo for Acme", None)
                .await
        );
    }
}
