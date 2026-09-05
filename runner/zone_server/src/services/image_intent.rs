//! Hybrid image-generation intent classification.
//!
//! High-confidence rules route immediately. Anything leftover — including
//! informal edits of an attached photo that the word lists miss — is decided
//! by a short LiteLLM call (`COMFYUI_CLASSIFIER_MODEL` / workspace `model_fast`,
//! default `llama3.2:3b`) with a small token budget. Timeouts and empty hosts
//! fall back to chat.

use serde_json::Value;
use std::time::Duration;
use zone_core::llm::{LlmClient, LlmConfig, Message};

use crate::config::ComfyUiConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleDecision {
    Image,
    Video,
    Chat,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationIntent {
    Chat,
    Image,
    Video,
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
    pub async fn classify(&self, content: &str, metadata: Option<&Value>) -> GenerationIntent {
        if !self.config.enabled {
            return GenerationIntent::Chat;
        }
        if metadata
            .and_then(|m| m.get("video_generation"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            return GenerationIntent::Video;
        }
        if metadata
            .and_then(|m| m.get("image_generation"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            return GenerationIntent::Image;
        }
        let skip_video = metadata
            .and_then(|m| m.get("video_generation"))
            .and_then(Value::as_bool)
            == Some(false);
        let skip_image = metadata
            .and_then(|m| m.get("image_generation"))
            .and_then(Value::as_bool)
            == Some(false);
        if skip_video && skip_image {
            return GenerationIntent::Chat;
        }

        let has_source_image = crate::services::image_source::has_image_attachment(metadata);
        match deterministic_decision(content, has_source_image) {
            RuleDecision::Video if !skip_video => GenerationIntent::Video,
            RuleDecision::Image if !skip_image => GenerationIntent::Image,
            RuleDecision::Ambiguous if !skip_image => {
                if self.classify_ambiguous(content, has_source_image).await {
                    GenerationIntent::Image
                } else {
                    GenerationIntent::Chat
                }
            }
            _ => GenerationIntent::Chat,
        }
    }

    pub async fn is_image_request(&self, content: &str, metadata: Option<&Value>) -> bool {
        self.classify(content, metadata).await == GenerationIntent::Image
    }

    async fn classify_ambiguous(&self, content: &str, has_source_image: bool) -> bool {
        if self.litellm_host.trim().is_empty() {
            return false;
        }
        let client = LlmClient::new(LlmConfig {
            base_url: self.litellm_host.clone(),
            api_key: self.litellm_key.clone(),
            default_model: self.config.classifier_model.clone(),
            temperature: 0.0,
            max_tokens: 3,
        });
        let prompt = if has_source_image {
            format!(
                "Return exactly IMAGE or CHAT. IMAGE when the user wants a new image generated now, \
                 or wants the attached image edited now: add, remove, replace, restyle, transform, \
                 change the background or environment, place the subject in a different setting, \
                 or any other change to the photo, including short or informal wording. \
                 Greetings, thanks, opinions, discussion, analysis, prompt-writing, coding, and \
                 questions about the attached image are CHAT.\nUser: {content}"
            )
        } else {
            format!(
                "Return exactly IMAGE or CHAT. IMAGE when the user wants a new image generated now, \
                 or wants something added to, removed from, or changed on an existing image now, \
                 including a new background or environment, even if the wording is informal. \
                 Discussion, analysis, prompt-writing, coding, and how-to questions are CHAT.\nUser: {content}"
            )
        };
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

    /// Turn an attached-image edit request into a CLIP prompt for img2img.
    /// Falls back to a heuristic if the classifier model is unavailable.
    pub async fn edit_prompt(&self, content: &str) -> String {
        let fallback = heuristic_edit_prompt(content);
        if self.litellm_host.trim().is_empty() {
            return fallback;
        }
        let client = LlmClient::new(LlmConfig {
            base_url: self.litellm_host.clone(),
            api_key: self.litellm_key.clone(),
            default_model: self.config.classifier_model.clone(),
            temperature: 0.2,
            max_tokens: 160,
        });
        let prompt = format!(
            "Rewrite the user's request as a positive prompt for an image model that starts from \
             the attached photo. Describe the finished photograph, not the editing instruction. \
             Keep the same main subject, identity, and pose unless the user asked to change them. \
             If they asked to remove something, describe the scene without it and with that area \
             filled in naturally; do not name the removed thing. If they asked to change the \
             environment or background, describe the same subject in that new setting. \
             No quotes, labels, or preamble. One or two sentences.\nUser: {content}"
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
            return fallback;
        };
        let Some(answer) = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
        else {
            return fallback;
        };
        sanitize_rewritten_prompt(answer, content).unwrap_or(fallback)
    }
}

fn deterministic_decision(content: &str, has_source_image: bool) -> RuleDecision {
    let tokens = tokenize(content);
    if tokens.is_empty() {
        return RuleDecision::Chat;
    }
    let has = |word: &str| tokens.iter().any(|token| token == word);
    let has_phrase = |phrase: &[&str]| phrase_in(&tokens, phrase);

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
    let analysis_request = ["describe", "analyze", "analyse", "inspect", "look"]
        .iter()
        .any(|word| has(word))
        && (has("image") || has("picture") || has("photo"));
    let prompt_request =
        has("prompt") && (has("write") || has("improve") || has_phrase(&["prompt", "for"]));
    let discussion_request = has_phrase(&["talk", "about"])
        || has_phrase(&["discuss", "image"])
        || (asks_how
            && (has("generate") || has("create") || has("make") || has("add") || has("remove")));
    if analysis_request || prompt_request || discussion_request || discusses_code || asks_how {
        return RuleDecision::Chat;
    }

    if is_video_request(&tokens, &has_phrase) {
        return RuleDecision::Video;
    }

    if is_edit_request(&tokens, &has_phrase)
        && (has_source_image || refers_to_existing_image(&tokens, &has_phrase))
    {
        return RuleDecision::Image;
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
        || has_source_image
    {
        // Attached photos: let the fast classifier catch informal edits the
        // word lists miss ("same subject at night", "without the chair").
        RuleDecision::Ambiguous
    } else {
        RuleDecision::Chat
    }
}

/// True when the prompt asks to change an image that is already on the thread.
pub fn should_reuse_thread_image(content: &str) -> bool {
    let tokens = tokenize(content);
    let has_phrase = |phrase: &[&str]| phrase_in(&tokens, phrase);
    match deterministic_decision(content, false) {
        RuleDecision::Image | RuleDecision::Video => {
            refers_to_existing_image(&tokens, &has_phrase)
                || is_animate_existing(&tokens, &has_phrase)
        }
        _ => false,
    }
}

fn tokenize(content: &str) -> Vec<String> {
    content
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn phrase_in(tokens: &[String], phrase: &[&str]) -> bool {
    tokens
        .windows(phrase.len())
        .any(|window| window.iter().map(String::as_str).eq(phrase.iter().copied()))
}

fn is_video_request(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    const ACTIONS: &[&str] = &["generate", "create", "make", "render", "animate"];
    const VIDEO_NOUNS: &[&str] = &[
        "video",
        "videos",
        "clip",
        "clips",
        "animation",
        "animations",
        "footage",
    ];
    let animate = tokens
        .iter()
        .take(4)
        .any(|token| token == "animate" || token == "animating");
    let explicit = tokens.iter().enumerate().any(|(index, token)| {
        ACTIONS.contains(&token.as_str())
            && tokens
                .iter()
                .skip(index + 1)
                .take(7)
                .any(|candidate| VIDEO_NOUNS.contains(&candidate.as_str()))
    });
    explicit
        || animate
        || has_phrase(&["text", "to", "video"])
        || has_phrase(&["image", "to", "video"])
        || has_phrase(&["make", "this", "move"])
        || has_phrase(&["make", "it", "move"])
        || has_phrase(&["make", "this", "a", "video"])
        || has_phrase(&["turn", "this", "into", "a", "video"])
        || has_phrase(&["bring", "this", "to", "life"])
}

fn is_animate_existing(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    has_phrase(&["animate", "this"])
        || has_phrase(&["animate", "it"])
        || has_phrase(&["make", "this", "move"])
        || has_phrase(&["make", "it", "move"])
        || has_phrase(&["make", "this", "a", "video"])
        || has_phrase(&["turn", "this", "into", "a", "video"])
        || (tokens.iter().any(|token| token == "animate")
            && refers_to_existing_image(tokens, has_phrase))
}

fn is_edit_request(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    const STRONG_EDITS: &[&str] = &[
        "add",
        "remove",
        "delete",
        "erase",
        "replace",
        "insert",
        "overlay",
        "crop",
        "inpaint",
        "outpaint",
        "recolor",
        "restyle",
        "remix",
        "redraw",
        "repaint",
        "reimagine",
        "edit",
        "transform",
        "modify",
        "convert",
        "wipe",
        "relocate",
    ];
    const WEAK_EDITS: &[&str] = &[
        "put", "place", "take", "fill", "hide", "fix", "clean", "clear", "brighten", "darken",
        "sharpen", "blur", "vary", "swap", "move",
    ];
    let weak_edit = tokens
        .iter()
        .any(|token| WEAK_EDITS.contains(&token.as_str()));
    tokens
        .iter()
        .any(|token| STRONG_EDITS.contains(&token.as_str()))
        || (weak_edit && refers_to_existing_image(tokens, has_phrase))
        || has_phrase(&["get", "rid"])
        || has_phrase(&["take", "out"])
        || has_phrase(&["take", "off"])
        || has_phrase(&["cut", "out"])
        || has_phrase(&["make", "this"])
        || has_phrase(&["make", "it"])
        || has_phrase(&["turn", "this"])
        || has_phrase(&["turn", "it"])
        || has_phrase(&["change", "this"])
        || has_phrase(&["change", "the"])
        || has_phrase(&["put", "this"])
        || has_phrase(&["place", "this"])
        || has_phrase(&["put", "it"])
        || has_phrase(&["place", "it"])
        || has_phrase(&["move", "this"])
        || has_phrase(&["different", "environment"])
        || has_phrase(&["new", "environment"])
        || has_phrase(&["another", "environment"])
        || has_phrase(&["different", "background"])
        || has_phrase(&["new", "background"])
        || has_phrase(&["different", "setting"])
        || has_phrase(&["new", "setting"])
        || has_phrase(&["another", "scene"])
        || has_phrase(&["based", "on", "this"])
        || has_phrase(&["from", "this"])
        || has_phrase(&["using", "this"])
        || has_phrase(&["to", "this"])
        || has_phrase(&["in", "this"])
        || has_phrase(&["on", "this"])
        || has_phrase(&["into", "this"])
}

fn refers_to_existing_image(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    has_phrase(&["this", "image"])
        || has_phrase(&["this", "picture"])
        || has_phrase(&["this", "photo"])
        || has_phrase(&["the", "image"])
        || has_phrase(&["the", "picture"])
        || has_phrase(&["the", "photo"])
        || has_phrase(&["that", "image"])
        || has_phrase(&["that", "picture"])
        || has_phrase(&["that", "photo"])
        || has_phrase(&["the", "attached"])
        || has_phrase(&["this", "object"])
        || has_phrase(&["the", "object"])
        || has_phrase(&["this", "subject"])
        || has_phrase(&["this", "one"])
        || has_phrase(&["from", "an", "image"])
        || has_phrase(&["from", "this"])
        || has_phrase(&["put", "this"])
        || has_phrase(&["place", "this"])
        || has_phrase(&["put", "it"])
        || has_phrase(&["place", "it"])
        || has_phrase(&["this", "background"])
        || has_phrase(&["the", "background"])
        || has_phrase(&["this", "watermark"])
        || has_phrase(&["the", "watermark"])
}

fn is_removal_request(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    ["remove", "delete", "erase", "wipe"]
        .iter()
        .any(|word| tokens.iter().any(|token| token == *word))
        || has_phrase(&["get", "rid"])
        || has_phrase(&["take", "out"])
        || has_phrase(&["take", "off"])
        || has_phrase(&["cut", "out"])
}

fn is_environment_change(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "environment" | "background" | "setting" | "scene" | "backdrop"
        )
    }) || has_phrase(&["put", "this"])
        || has_phrase(&["place", "this"])
        || has_phrase(&["put", "it"])
        || has_phrase(&["place", "it"])
        || has_phrase(&["in", "a"])
        || has_phrase(&["into", "a"])
}

/// CLIP text for img2img when the classifier model cannot rewrite the request.
pub fn heuristic_edit_prompt(content: &str) -> String {
    let tokens = tokenize(content);
    let has_phrase = |phrase: &[&str]| phrase_in(&tokens, phrase);
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "the same subject, edited as requested, photorealistic".to_string();
    }
    if is_removal_request(&tokens, &has_phrase) {
        format!(
            "the same photograph with the requested object gone, that area filled in naturally \
             to match the surrounding scene, no leftover object or hole, photorealistic. {trimmed}"
        )
    } else if is_environment_change(&tokens, &has_phrase) {
        format!(
            "the same subject in the new environment described, keep the subject's identity, \
             pose, and appearance, only change the setting, matching lighting, photorealistic. \
             {trimmed}"
        )
    } else {
        format!(
            "the same subject with the requested edits applied, keep identity and composition \
             unless asked to change them, photorealistic. {trimmed}"
        )
    }
}

fn sanitize_rewritten_prompt(answer: &str, original: &str) -> Option<String> {
    let mut text = answer.trim().trim_matches('"').trim_matches('\'').trim();
    if let Some(stripped) = text.strip_prefix("Prompt:") {
        text = stripped.trim();
    }
    if text.is_empty()
        || text.len() > 4_000
        || text.eq_ignore_ascii_case("IMAGE")
        || text.eq_ignore_ascii_case("CHAT")
        || text.eq_ignore_ascii_case("VIDEO")
    {
        return None;
    }
    if text.eq_ignore_ascii_case(original.trim()) {
        return Some(heuristic_edit_prompt(original));
    }
    Some(format!("{text} {}", original.trim()))
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
            "Generate an image of the same rooster facing the other way",
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
                deterministic_decision(request, false),
                RuleDecision::Image,
                "{request}"
            );
        }
        for video in [
            "Generate a video of a red panda",
            "Create a clip of waves crashing",
            "Make me a video of a lighthouse",
            "Animate this image",
            "Turn this into a video",
            "image to video of this photo",
            "text to video of a fox running",
            "Make this move",
        ] {
            assert_eq!(
                deterministic_decision(video, video.contains("this") || video.contains("photo")),
                RuleDecision::Video,
                "{video}"
            );
        }
        for edit in [
            "Make this a watercolor",
            "Turn this into a painting",
            "Edit this image to add a sunset",
            "Restyle this as cyberpunk",
            "Change the background to a forest",
            "Remove from this image",
            "Add to this image",
            "Add a hat to this picture",
            "Remove the person from this photo",
            "Delete the text in this image",
            "Erase the watermark",
            "Put a sunset in this photo",
            "Take the logo off this image",
            "Get rid of the background",
            "Replace the sky in this picture",
            "Put this object in a different environment",
            "Remove this object from an image",
            "Place this on a beach",
            "Put this in a snowy forest",
        ] {
            assert_eq!(
                deterministic_decision(edit, true),
                RuleDecision::Image,
                "{edit}"
            );
        }
        for needs_source in [
            "Make this a watercolor",
            "Turn this into a painting",
            "Restyle this as cyberpunk",
        ] {
            assert_ne!(
                deterministic_decision(needs_source, false),
                RuleDecision::Image,
                "{needs_source}"
            );
        }
        for named_image in [
            "Remove from this image",
            "Add to this image",
            "Add a hat to this picture",
            "Edit this image to add a sunset",
            "Change the background to a forest",
            "Put this object in a different environment",
            "Remove this object from an image",
            "Place this on a beach",
        ] {
            assert_eq!(
                deterministic_decision(named_image, false),
                RuleDecision::Image,
                "{named_image}"
            );
            assert!(should_reuse_thread_image(named_image), "{named_image}");
        }
        assert!(!should_reuse_thread_image(
            "Generate an image of a blue fox"
        ));
        assert!(!should_reuse_thread_image(
            "Generate an image of the environment"
        ));
        assert!(!should_reuse_thread_image(
            "Generate an image of a wolf with a snowy background"
        ));
        assert!(should_reuse_thread_image(
            "Change the background to a forest"
        ));
        assert_eq!(
            deterministic_decision("please take a look, can you fix it?", true),
            RuleDecision::Ambiguous
        );
        assert_eq!(
            deterministic_decision("please take a look, can you fix it?", false),
            RuleDecision::Chat
        );
        assert!(should_reuse_thread_image("Animate this image"));
        assert!(should_reuse_thread_image("Make this a video"));
        assert!(!should_reuse_thread_image("Generate a video of a fox"));
        assert!(!should_reuse_thread_image(
            "How do I add a hat to this image?"
        ));
        assert_eq!(
            deterministic_decision("How do I add a hat to this image?", true),
            RuleDecision::Chat
        );
        assert_eq!(
            deterministic_decision("Add a hat", false),
            RuleDecision::Chat
        );
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
            assert_eq!(
                deterministic_decision(chat, false),
                RuleDecision::Chat,
                "{chat}"
            );
        }
        assert_eq!(
            deterministic_decision("Describe this image", true),
            RuleDecision::Chat
        );
        assert_eq!(
            deterministic_decision("Could you design a logo for Acme?", false),
            RuleDecision::Ambiguous
        );
        assert_eq!(
            deterministic_decision("the same subject at night", false),
            RuleDecision::Chat
        );
        assert_eq!(
            deterministic_decision("the same subject at night", true),
            RuleDecision::Ambiguous
        );
        assert_eq!(
            deterministic_decision("without the chair", true),
            RuleDecision::Ambiguous
        );
        assert_eq!(
            deterministic_decision("thanks", true),
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

    #[test]
    fn heuristic_edit_prompt_rewrites_environment_and_removal() {
        let environment = heuristic_edit_prompt("Put this object in a different environment");
        assert!(environment.contains("new environment"));
        assert!(environment.contains("Put this object in a different environment"));

        let beach = heuristic_edit_prompt("Place this on a beach");
        assert!(beach.contains("new environment"));
        assert!(beach.contains("Place this on a beach"));

        let removal = heuristic_edit_prompt("Remove this object from an image");
        assert!(removal.contains("requested object gone"));
        assert!(removal.contains("Remove this object from an image"));

        let style = heuristic_edit_prompt("Make this a watercolor");
        assert!(style.contains("requested edits"));
        assert!(style.contains("Make this a watercolor"));
    }

    #[test]
    fn sanitize_rewritten_prompt_keeps_original_instruction() {
        let rewritten = sanitize_rewritten_prompt(
            "A wooden chair on a misty forest path, photorealistic.",
            "Put this object in a different environment",
        )
        .unwrap();
        assert!(rewritten.contains("wooden chair on a misty forest path"));
        assert!(rewritten.contains("Put this object in a different environment"));

        assert!(sanitize_rewritten_prompt("IMAGE", "remove the chair").is_none());
        let echoed = sanitize_rewritten_prompt(
            "Remove this object from an image",
            "Remove this object from an image",
        )
        .unwrap();
        assert!(echoed.contains("requested object gone"));
    }

    #[tokio::test]
    async fn edit_prompt_uses_rewritten_scene_and_keeps_original() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "rewrite",
                "object": "chat.completion",
                "created": 0,
                "model": "fast",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "A wooden chair on a misty forest path, photorealistic."
                    },
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
        let prompt = classifier
            .edit_prompt("Put this object in a different environment")
            .await;
        assert!(prompt.contains("wooden chair on a misty forest path"));
        assert!(prompt.contains("Put this object in a different environment"));
    }

    #[tokio::test]
    async fn edit_prompt_falls_back_when_host_is_empty() {
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                ..Default::default()
            },
            String::new(),
            String::new(),
        );
        let prompt = classifier
            .edit_prompt("Remove this object from an image")
            .await;
        assert!(prompt.contains("requested object gone"));
        assert!(prompt.contains("Remove this object from an image"));
    }

    fn attached_png() -> serde_json::Value {
        serde_json::json!({
            "attachments": [{
                "name": "photo.png",
                "mime": "image/png",
                "url": "data:image/png;base64,aaa"
            }]
        })
    }

    #[tokio::test]
    async fn attached_informal_edit_uses_fast_classifier() {
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
                .is_image_request("the same subject at night", Some(&attached_png()))
                .await
        );
    }

    #[tokio::test]
    async fn attached_thanks_stays_chat_when_classifier_says_chat() {
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
                    "message": {"role": "assistant", "content": "CHAT"},
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
            !classifier
                .is_image_request("thanks", Some(&attached_png()))
                .await
        );
    }

    #[tokio::test]
    async fn attached_informal_edit_stays_chat_without_classifier_host() {
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                ..Default::default()
            },
            String::new(),
            String::new(),
        );
        assert!(
            !classifier
                .is_image_request("the same subject at night", Some(&attached_png()))
                .await
        );
    }
}
