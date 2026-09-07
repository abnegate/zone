//! Vision captioning for LoRA training sets.
//!
//! An identity LoRA learns its subject from the trigger word, so a caption must
//! describe only what changes between shots. A caption that restates the subject
//! teaches the model to rebuild it from the words instead, and prompting the
//! trigger alone then renders something unrelated. Captioning therefore runs in
//! two passes: name the subject shared by every image, then describe each image
//! while excluding that subject.

use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use zone_core::llm::{LlmClient, LlmConfig, Message};

use crate::config::ComfyUiConfig;

const SUBJECT_SAMPLE: usize = 6;
const SUBJECT_TOKENS: u32 = 60;
const DESCRIPTION_TOKENS: u32 = 80;
const MAX_CAPTION_WORDS: usize = 18;

const PREAMBLES: &[&str] = &[
    "in this image,",
    "in the image,",
    "in this photo,",
    "in the photo,",
    "this image shows",
    "this image depicts",
    "this photo shows",
    "the image shows",
    "the image depicts",
    "the photo shows",
    "here is",
    "this is",
    "a photo of",
    "a picture of",
    "an image of",
    "image of",
    "picture of",
    "photo of",
];

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "the", "with", "of", "in", "on", "at", "for", "to", "its", "it", "that",
    "this", "has", "have", "is", "are", "was", "were", "one", "two", "some", "very", "small",
    "large", "big", "tiny",
];

#[derive(Debug, Deserialize)]
pub struct CaptionRequest {
    #[serde(default)]
    pub trigger: Option<String>,
    pub images: Vec<CaptionImage>,
}

#[derive(Debug, Deserialize)]
pub struct CaptionImage {
    pub filename: String,
    pub bytes_base64: String,
    #[serde(default)]
    pub caption: String,
}

/// Inline data URL, the only image shape an OpenAI-compatible vision model takes.
pub fn data_url(filename: &str, base64: &str) -> String {
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let mime = match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    };
    format!("data:{mime};base64,{}", base64.trim())
}

pub struct Captioner {
    model: String,
    timeout: Duration,
    host: String,
    key: String,
}

impl Captioner {
    pub fn new(config: &ComfyUiConfig, litellm_host: String, litellm_key: String) -> Self {
        Self {
            model: config.caption_model.clone(),
            timeout: Duration::from_secs(config.caption_timeout_secs),
            host: litellm_host,
            key: litellm_key,
        }
    }

    pub fn available(&self) -> bool {
        !self.host.trim().is_empty() && !self.model.trim().is_empty()
    }

    /// Caption every image whose caption is blank. User-written captions are kept.
    pub async fn fill(&self, images: &mut [(String, String)], trigger: &str) {
        if !self.available() || images.iter().all(|(_, caption)| !caption.trim().is_empty()) {
            return;
        }
        let urls: Vec<String> = images.iter().map(|(url, _)| url.clone()).collect();
        let subject = self.subject(&urls).await;
        for (url, caption) in images.iter_mut() {
            if !caption.trim().is_empty() {
                continue;
            }
            let described = self.describe(url, subject.as_deref()).await;
            *caption = compose(trigger, described.as_deref(), subject.as_deref());
        }
    }

    /// One noun phrase for the subject every image shares, used as an exclusion list.
    async fn subject(&self, images: &[String]) -> Option<String> {
        let prompt = "These images all show the same subject. Reply with one short noun phrase \
             naming that subject and the visual traits it keeps in every image. \
             No sentence, no trailing period, at most 20 words.";
        let mut message = Message::user(prompt);
        message.images = images.iter().take(SUBJECT_SAMPLE).cloned().collect();
        let answer = self.ask(message, SUBJECT_TOKENS).await?;
        let cleaned = tidy(&answer);
        (!cleaned.is_empty()).then_some(cleaned)
    }

    /// What varies in one image: pose, framing, setting, lighting, props.
    async fn describe(&self, image: &str, subject: Option<&str>) -> Option<String> {
        let exclusion = match subject {
            Some(subject) => {
                format!(" Never describe the subject itself. Do not mention: {subject}.")
            }
            None => String::new(),
        };
        let prompt = format!(
            "Describe only what changes between photos of this subject: camera angle, framing, \
             pose or action, background or setting, lighting, and any clothing or props.{exclusion} \
             Reply with lowercase comma-separated fragments, 4 to 14 words, no full sentence, \
             and no leading phrase such as \"the image shows\"."
        );
        let mut message = Message::user(prompt);
        message.images = vec![image.to_string()];
        let answer = self.ask(message, DESCRIPTION_TOKENS).await?;
        let cleaned = tidy(&answer);
        (!cleaned.is_empty()).then_some(cleaned)
    }

    async fn ask(&self, message: Message, max_tokens: u32) -> Option<String> {
        let client = LlmClient::new(LlmConfig {
            base_url: self.host.clone(),
            api_key: self.key.clone(),
            default_model: self.model.clone(),
            temperature: 0.0,
            max_tokens,
        });
        let result = tokio::time::timeout(
            self.timeout,
            client.chat_with_model(&self.model, &[message], None),
        )
        .await;
        let Ok(Ok(response)) = result else {
            return None;
        };
        response
            .choices
            .first()
            .and_then(|choice| choice.message.content.clone())
    }
}

/// Trigger plus the variable description, with any subject leakage removed.
pub fn compose(trigger: &str, description: Option<&str>, subject: Option<&str>) -> String {
    let trigger = trigger.trim();
    let Some(description) = description.map(|value| without_subject(value, subject)) else {
        return trigger.to_string();
    };
    if description.is_empty() {
        return trigger.to_string();
    }
    if trigger.is_empty() {
        return description;
    }
    format!("{trigger}, {description}")
}

/// Drop comma clauses that name the shared subject, so an ignored instruction
/// cannot put identity words back into the caption.
fn without_subject(description: &str, subject: Option<&str>) -> String {
    let banned: HashSet<String> = subject
        .map(|subject| {
            subject
                .split(|character: char| !character.is_alphanumeric())
                .map(str::to_ascii_lowercase)
                .filter(|word| word.len() > 2 && !STOPWORDS.contains(&word.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let kept: Vec<&str> = description
        .split(',')
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .filter(|clause| {
            !clause
                .split(|character: char| !character.is_alphanumeric())
                .any(|word| banned.contains(&word.to_ascii_lowercase()))
        })
        .collect();
    truncate_words(&kept.join(", "), MAX_CAPTION_WORDS)
}

/// Strip model chatter, markdown, and preambles from a raw answer.
fn tidy(answer: &str) -> String {
    let mut value = answer.trim().replace(['\n', '\r'], " ");
    value = value.replace(['*', '`', '"'], "");
    value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut lowered = value.to_ascii_lowercase();
    loop {
        let trimmed = PREAMBLES
            .iter()
            .find(|preamble| lowered.starts_with(**preamble))
            .map(|preamble| preamble.len());
        match trimmed {
            Some(length) => {
                value = value[length..].trim_start().to_string();
                lowered = value.to_ascii_lowercase();
            }
            None => break,
        }
    }
    value
        .trim_end_matches(['.', ',', ';'])
        .trim()
        .trim_start_matches(['-', ':'])
        .trim()
        .to_string()
}

fn truncate_words(value: &str, limit: usize) -> String {
    let words: Vec<&str> = value.split_whitespace().collect();
    if words.len() <= limit {
        return value.trim_end_matches(',').to_string();
    }
    words[..limit].join(" ").trim_end_matches(',').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn config(model: &str) -> ComfyUiConfig {
        ComfyUiConfig {
            caption_model: model.to_string(),
            caption_timeout_secs: 5,
            ..Default::default()
        }
    }

    fn answer(content: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "caption",
            "object": "chat.completion",
            "created": 0,
            "model": "vision",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": "stop"
            }]
        })
    }

    #[test]
    fn tidy_strips_preamble_markdown_and_trailing_punctuation() {
        assert_eq!(
            tidy("The image shows **a robot on concrete**."),
            "a robot on concrete"
        );
        assert_eq!(
            tidy("In this image, standing outdoors;"),
            "standing outdoors"
        );
        assert_eq!(
            tidy("\"a photo of side profile, walking\""),
            "side profile, walking"
        );
    }

    #[test]
    fn subject_words_are_removed_from_captions() {
        let subject = "a lime-green cube-headed robot with a glossy red teapot body";
        let description = "lime-green cube-headed robot, three-quarter view, standing on concrete";
        assert_eq!(
            without_subject(description, Some(subject)),
            "three-quarter view, standing on concrete"
        );
    }

    #[test]
    fn caption_keeps_only_the_trigger_when_every_clause_leaks() {
        let subject = "a red teapot robot";
        let caption = compose(
            "zrkxyz",
            Some("a red teapot robot, teapot robot closeup"),
            Some(subject),
        );
        assert_eq!(caption, "zrkxyz");
    }

    #[test]
    fn caption_prefixes_trigger_and_caps_length() {
        let long = (0..40)
            .map(|index| format!("word{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let caption = compose("zrkxyz", Some(&long), None);
        assert!(caption.starts_with("zrkxyz, "));
        assert_eq!(caption.split_whitespace().count(), MAX_CAPTION_WORDS + 1);
    }

    #[tokio::test]
    async fn fill_captions_blank_entries_and_keeps_written_ones() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("all show the same subject"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer(
                "a lime-green cube-headed robot with a glossy red teapot body",
            )))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("what changes between photos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer(
                "The image shows a lime-green teapot robot, three-quarter view, standing on concrete.",
            )))
            .mount(&server)
            .await;
        let captioner = Captioner::new(&config("vision"), server.uri(), "key".into());
        let mut images = vec![
            ("data:image/png;base64,aaa".to_string(), String::new()),
            (
                "data:image/png;base64,bbb".to_string(),
                "hand written".to_string(),
            ),
        ];
        captioner.fill(&mut images, "zrkxyz").await;

        assert_eq!(images[1].1, "hand written", "written captions must survive");
        let generated = &images[0].1;
        assert!(generated.starts_with("zrkxyz, "), "got {generated}");
        assert!(
            !generated.contains("teapot") && !generated.contains("robot"),
            "subject leaked into {generated}"
        );
        assert!(
            generated.contains("concrete"),
            "context dropped from {generated}"
        );
    }

    #[tokio::test]
    async fn fill_is_a_no_op_without_a_caption_model() {
        let captioner = Captioner::new(&config(""), "http://unused".into(), "key".into());
        let mut images = vec![("data:image/png;base64,aaa".to_string(), String::new())];
        captioner.fill(&mut images, "zrkxyz").await;
        assert_eq!(images[0].1, "");
    }

    #[tokio::test]
    async fn unreachable_vision_model_leaves_the_trigger_only_caption() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let captioner = Captioner::new(&config("vision"), server.uri(), "key".into());
        let mut images = vec![("data:image/png;base64,aaa".to_string(), String::new())];
        captioner.fill(&mut images, "zrkxyz").await;
        assert_eq!(images[0].1, "zrkxyz");
    }
}
