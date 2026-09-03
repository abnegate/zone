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
        if !self.config.enabled {
            return false;
        }
        if let Some(force) = metadata
            .and_then(|m| m.get("image_generation"))
            .and_then(Value::as_bool)
        {
            return force;
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
    let tokens: Vec<String> = content
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if tokens.is_empty() {
        return RuleDecision::Chat;
    }
    let has = |word: &str| tokens.iter().any(|token| token == word);
    let has_phrase = |phrase: &[&str]| {
        tokens
            .windows(phrase.len())
            .any(|window| window.iter().map(String::as_str).eq(phrase.iter().copied()))
    };

    // High-confidence non-generation intents take precedence. Exact tokens
    // avoid treating "a teacher explaining relativity" as a request to
    // explain image generation.
    let asks_how = has_phrase(&["how", "to"]) || has_phrase(&["how", "do", "i"]);
    let programming_language = ["react", "typescript", "javascript", "rust", "python"]
        .iter()
        .any(|word| has(word));
    let implementation_term = [
        "component",
        "function",
        "class",
        "api",
        "workflow",
        "code",
        "implement",
        "html",
        "css",
    ]
    .iter()
    .any(|word| has(word));
    let discusses_code = (programming_language && implementation_term)
        || ((has("image") || has("images"))
            && ["component", "api", "workflow", "code", "implement"]
                .iter()
                .any(|word| has(word)));
    let analysis_request = ["describe", "analyze", "analyse", "inspect"]
        .iter()
        .any(|word| has(word))
        && (has("image") || has("picture") || has("photo"));
    let prompt_request =
        has("prompt") && (has("write") || has("improve") || has_phrase(&["prompt", "for"]));
    let discussion_request = has_phrase(&["talk", "about"])
        || has_phrase(&["discuss", "image"])
        || (asks_how && (has("generate") || has("create") || has("make")));
    if analysis_request || prompt_request || discussion_request || discusses_code {
        return RuleDecision::Chat;
    }

    const ACTIONS: &[&str] = &[
        "generate",
        "create",
        "make",
        "draw",
        "render",
        "paint",
        "illustrate",
        "sketch",
    ];
    const VISUAL_NOUNS: &[&str] = &[
        "image",
        "images",
        "picture",
        "pictures",
        "photo",
        "photos",
        "artwork",
        "illustration",
        "illustrations",
        "poster",
        "posters",
        "logo",
        "logos",
        "wallpaper",
        "wallpapers",
        "portrait",
        "portraits",
    ];
    let explicit = tokens.iter().enumerate().any(|(index, token)| {
        ACTIONS.contains(&token.as_str())
            && tokens
                .iter()
                .skip(index + 1)
                .take(7)
                .any(|candidate| VISUAL_NOUNS.contains(&candidate.as_str()))
    });
    let visual_imperative = tokens
        .iter()
        .take(4)
        .any(|token| ["draw", "paint", "illustrate", "sketch"].contains(&token.as_str()))
        && !["conclusion", "conclusions", "attention", "parallel"]
            .iter()
            .any(|word| has(word));
    if explicit || visual_imperative {
        return RuleDecision::Image;
    }

    if tokens
        .iter()
        .any(|token| ACTIONS.contains(&token.as_str()) || VISUAL_NOUNS.contains(&token.as_str()))
        || has("visualize")
        || has("visualise")
    {
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
            "Generate an image of a teacher explaining relativity",
            "Make me three images of red pandas",
            "Create pictures showing the four seasons",
            "Generate an image of a Python snake",
            "generate images",
            "make me an image",
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
            "How should I create a React image component?",
            "Discuss image components in React",
            "Render an image component in React",
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
            !disabled
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
