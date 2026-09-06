//! Optional per-stage model selection.
//!
//! Workspace/org settings and `OLLAMA_MODEL_*` / `COMFYUI_*` env vars can pin
//! every stage. When they are empty, chat messages pick from the models that
//! are actually installed.

use reqwest::Client;
use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;

use crate::db::ai_settings::EffectiveAiSettings;

pub const AUTO: &str = "auto";

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(2))
        .build()
        .expect("Failed to build model catalog client")
});

#[derive(Debug, Clone, Default)]
pub struct Preferences {
    pub fast: Option<String>,
    pub reasoning: Option<String>,
    pub embedding: Option<String>,
    pub vision: Option<String>,
    pub classifier: Option<String>,
}

impl Preferences {
    pub fn from_settings(settings: &EffectiveAiSettings, classifier: &str) -> Self {
        Self {
            fast: nonempty(settings.model_fast.as_deref())
                .or_else(|| env_optional("OLLAMA_MODEL_FAST")),
            reasoning: nonempty(settings.model_reasoning.as_deref())
                .or_else(|| env_optional("OLLAMA_MODEL_REASON")),
            embedding: nonempty(settings.model_embedding.as_deref())
                .or_else(|| env_optional("OLLAMA_MODEL_EMBED")),
            vision: env_optional("OLLAMA_MODEL_VISION"),
            classifier: nonempty(settings.model_fast.as_deref())
                .or_else(|| nonempty(Some(classifier)))
                .or_else(|| env_optional("COMFYUI_CLASSIFIER_MODEL")),
        }
    }

    pub fn from_optional_settings(
        settings: Option<&EffectiveAiSettings>,
        classifier: &str,
    ) -> Self {
        match settings {
            Some(settings) => Self::from_settings(settings, classifier),
            None => Self {
                classifier: nonempty(Some(classifier)),
                ..Self::default()
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub name: String,
    pub bytes: u64,
    pub million_params: Option<u32>,
    pub embedding: bool,
    pub vision: bool,
    pub reranker: bool,
}

impl Installed {
    pub fn completion(&self) -> bool {
        !self.embedding && !self.reranker
    }
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub models: Vec<Installed>,
}

impl Catalog {
    pub async fn load(ollama_host: &str) -> Self {
        let url = format!("{}/api/tags", ollama_host.trim_end_matches('/'));
        let Ok(response) = CLIENT.get(url).send().await else {
            return Self::default();
        };
        let Ok(tags) = response.error_for_status() else {
            return Self::default();
        };
        let Ok(body) = tags.json::<Tags>().await else {
            return Self::default();
        };
        Self {
            models: body.models.into_iter().map(Installed::from).collect(),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    pub fn find(&self, name: &str) -> Option<&Installed> {
        self.models
            .iter()
            .find(|model| same_model(&model.name, name))
    }

    fn completions(&self) -> impl Iterator<Item = &Installed> {
        self.models.iter().filter(|model| model.completion())
    }
}

/// True when the chat should pick a stage from the message instead of a pin.
pub fn is_auto(name: &str) -> bool {
    let name = name.trim();
    name.is_empty() || name.eq_ignore_ascii_case(AUTO)
}

/// Chat completion model for this message.
pub fn chat_model(
    requested: &str,
    prefs: &Preferences,
    catalog: &Catalog,
    message: &str,
    has_image: bool,
    agent: bool,
) -> String {
    if !is_auto(requested) {
        return requested.to_string();
    }
    if has_image
        && let Some(model) = pick_installed(prefs.vision.as_deref(), catalog, Stage::Vision)
    {
        return model;
    }
    if wants_reason(message)
        && let Some(model) = pick_installed(prefs.reasoning.as_deref(), catalog, Stage::Reason)
    {
        return model;
    }
    if agent && let Some(model) = pick_installed(prefs.fast.as_deref(), catalog, Stage::Fast) {
        return model;
    }
    pick_installed(prefs.fast.as_deref(), catalog, Stage::Fast)
        .or_else(|| pick_installed(prefs.reasoning.as_deref(), catalog, Stage::Reason))
        .unwrap_or_else(|| fallback_name(requested, prefs.fast.as_deref()))
}

/// LiteLLM model used to classify image intent and to title chats.
pub fn classifier_model(prefs: &Preferences, catalog: &Catalog, chat_model: &str) -> String {
    if let Some(name) = prefs.classifier.as_deref().filter(|name| !is_auto(name))
        && catalog_allows(catalog, name)
    {
        return name.to_string();
    }
    if !is_auto(chat_model) && catalog.contains(chat_model) {
        return chat_model.to_string();
    }
    pick_installed(prefs.fast.as_deref(), catalog, Stage::Fast)
        .or_else(|| {
            catalog
                .completions()
                .min_by_key(|model| (model.million_params.unwrap_or(u32::MAX), model.bytes))
                .map(|model| model.name.clone())
        })
        .unwrap_or_else(|| fallback_name(AUTO, prefs.classifier.as_deref()))
}

#[derive(Clone, Copy)]
enum Stage {
    Fast,
    Reason,
    Vision,
}

fn pick_installed(preferred: Option<&str>, catalog: &Catalog, stage: Stage) -> Option<String> {
    if let Some(name) = preferred.filter(|name| !is_auto(name))
        && catalog_allows(catalog, name)
    {
        return Some(name.to_string());
    }
    if catalog.models.is_empty() {
        return preferred.filter(|name| !is_auto(name)).map(str::to_string);
    }
    let mut candidates: Vec<&Installed> = match stage {
        Stage::Vision => catalog.models.iter().filter(|model| model.vision).collect(),
        Stage::Fast | Stage::Reason => catalog.completions().collect(),
    };
    if candidates.is_empty() {
        return None;
    }
    match stage {
        Stage::Reason => {
            candidates.sort_by_key(|model| {
                (
                    !reason_name(&model.name),
                    std::cmp::Reverse(model.million_params.unwrap_or(0)),
                    std::cmp::Reverse(model.bytes),
                )
            });
        }
        Stage::Fast => {
            candidates.sort_by_key(|model| {
                (
                    reason_name(&model.name),
                    model.million_params.unwrap_or(u32::MAX),
                    model.bytes,
                )
            });
        }
        Stage::Vision => {
            candidates.sort_by_key(|model| model.bytes);
        }
    }
    candidates.first().map(|model| model.name.clone())
}

fn catalog_allows(catalog: &Catalog, name: &str) -> bool {
    catalog.models.is_empty() || catalog.contains(name)
}

fn fallback_name(requested: &str, preferred: Option<&str>) -> String {
    if !is_auto(requested) {
        return requested.to_string();
    }
    preferred
        .filter(|name| !is_auto(name))
        .unwrap_or(AUTO)
        .to_string()
}

fn wants_reason(message: &str) -> bool {
    let text = message.to_ascii_lowercase();
    [
        "prove ",
        "derive ",
        "theorem",
        "complexity",
        "debug this",
        "root cause",
        "tradeoff",
        "trade-off",
        "audit",
        "formal proof",
        "step by step",
        "step-by-step",
        "architect",
        "distributed system",
        "migration plan",
        "security review",
        "why is this wrong",
        "edge cases",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn reason_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("r1")
        || name.contains("reason")
        || name.contains("qwq")
        || name.contains("think")
        || name.contains("deepseek")
}

fn same_model(left: &str, right: &str) -> bool {
    fn strip(name: &str) -> &str {
        name.strip_suffix(":latest")
            .or_else(|| name.strip_suffix(":LATEST"))
            .unwrap_or(name)
    }
    strip(left).eq_ignore_ascii_case(strip(right))
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case(AUTO))
        .map(str::to_string)
}

fn env_optional(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl From<Tag> for Installed {
    fn from(tag: Tag) -> Self {
        let lower = tag.name.to_ascii_lowercase();
        Self {
            million_params: tag
                .details
                .as_ref()
                .and_then(|details| parse_params(details.parameter_size.as_deref()))
                .or_else(|| parse_params(Some(&tag.name))),
            embedding: lower.contains("embed"),
            vision: lower.contains("llava")
                || lower.contains("vision")
                || lower.contains("minicpm-v"),
            reranker: lower.contains("rerank"),
            name: tag.name,
            bytes: tag.size,
        }
    }
}

fn parse_params(value: Option<&str>) -> Option<u32> {
    let value = value?;
    let lowered = value.to_ascii_lowercase();
    let candidate = lowered
        .rsplit_once(':')
        .map(|(_, tag)| tag)
        .unwrap_or(&lowered);
    let start = candidate.find(|ch: char| ch.is_ascii_digit())?;
    let rest = &candidate[start..];
    let end = rest
        .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .unwrap_or(rest.len());
    let number: f64 = rest[..end].parse().ok()?;
    let scale = rest[end..].trim_start_matches(|ch: char| !ch.is_ascii_alphabetic());
    let million = if scale.starts_with('b') {
        number * 1000.0
    } else if scale.starts_with('m') {
        number
    } else if lowered.contains(':') && number < 100.0 {
        number * 1000.0
    } else {
        return None;
    };
    Some(million.round() as u32)
}

#[derive(Deserialize)]
struct Tags {
    models: Vec<Tag>,
}

#[derive(Deserialize)]
struct Tag {
    name: String,
    #[serde(default)]
    size: u64,
    details: Option<TagDetails>,
}

#[derive(Deserialize)]
struct TagDetails {
    parameter_size: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(name: &str, bytes: u64, million_params: u32) -> Installed {
        let lower = name.to_ascii_lowercase();
        Installed {
            name: name.to_string(),
            bytes,
            million_params: Some(million_params),
            embedding: lower.contains("embed"),
            vision: lower.contains("llava"),
            reranker: lower.contains("rerank"),
        }
    }

    fn catalog(models: &[Installed]) -> Catalog {
        Catalog {
            models: models.to_vec(),
        }
    }

    fn prefs(fast: Option<&str>, reason: Option<&str>) -> Preferences {
        Preferences {
            fast: fast.map(str::to_string),
            reasoning: reason.map(str::to_string),
            embedding: None,
            vision: Some("llava:7b".to_string()),
            classifier: fast.map(str::to_string),
        }
    }

    #[test]
    fn auto_chat_uses_smallest_installed_completion_model() {
        let catalog = catalog(&[
            installed("nomic-embed-text", 274, 137),
            installed("qwen3.8:27b", 17_000, 27_000),
            installed("llama3.2:3b", 2_000, 3_000),
            installed("llava:7b", 4_700, 7_000),
        ]);
        assert_eq!(
            chat_model(AUTO, &prefs(None, None), &catalog, "hello", false, false),
            "llama3.2:3b"
        );
    }

    #[test]
    fn pinned_fast_wins_when_installed() {
        let catalog = catalog(&[
            installed("llama3.2:3b", 2_000, 3_000),
            installed("qwen3.8:27b", 17_000, 27_000),
        ]);
        assert_eq!(
            chat_model(
                AUTO,
                &prefs(Some("qwen3.8:27b"), None),
                &catalog,
                "hello",
                false,
                false
            ),
            "qwen3.8:27b"
        );
    }

    #[test]
    fn missing_pin_falls_back_to_installed() {
        let catalog = catalog(&[installed("llama3.2:3b", 2_000, 3_000)]);
        assert_eq!(
            chat_model(
                AUTO,
                &prefs(Some("llama3.1:8b"), Some("llama3.1:8b")),
                &catalog,
                "hello",
                false,
                false
            ),
            "llama3.2:3b"
        );
    }

    #[test]
    fn explicit_chat_model_is_kept() {
        let catalog = catalog(&[installed("llama3.2:3b", 2_000, 3_000)]);
        assert_eq!(
            chat_model(
                "qwen3.8:27b",
                &prefs(Some("llama3.2:3b"), None),
                &catalog,
                "hello",
                false,
                false
            ),
            "qwen3.8:27b"
        );
    }

    #[test]
    fn reasoning_prompt_picks_the_larger_model() {
        let catalog = catalog(&[
            installed("llama3.2:3b", 2_000, 3_000),
            installed("qwen3.8:27b", 17_000, 27_000),
        ]);
        assert_eq!(
            chat_model(
                AUTO,
                &prefs(None, None),
                &catalog,
                "Prove that this algorithm is correct and list edge cases",
                false,
                false
            ),
            "qwen3.8:27b"
        );
    }

    #[test]
    fn classifier_uses_chat_model_when_fast_is_unset() {
        let catalog = catalog(&[
            installed("llama3.2:3b", 2_000, 3_000),
            installed("qwen3.8:27b", 17_000, 27_000),
        ]);
        assert_eq!(
            classifier_model(&prefs(None, None), &catalog, "qwen3.8:27b"),
            "qwen3.8:27b"
        );
    }

    #[test]
    fn classifier_skips_uninstalled_env_pin() {
        let catalog = catalog(&[installed("llama3.2:3b", 2_000, 3_000)]);
        let prefs = Preferences {
            fast: None,
            reasoning: None,
            embedding: None,
            vision: None,
            classifier: Some("llama3.1:8b".to_string()),
        };
        assert_eq!(classifier_model(&prefs, &catalog, AUTO), "llama3.2:3b");
    }

    #[test]
    fn vision_stage_uses_llava_for_attached_images() {
        let catalog = catalog(&[
            installed("llama3.2:3b", 2_000, 3_000),
            installed("llava:7b", 4_700, 7_000),
        ]);
        assert_eq!(
            chat_model(
                AUTO,
                &prefs(None, None),
                &catalog,
                "what is this",
                true,
                false
            ),
            "llava:7b"
        );
    }

    #[test]
    fn name_tags_parse_as_billions_of_parameters() {
        assert_eq!(parse_params(Some("llama3.2:3b")), Some(3_000));
        assert_eq!(parse_params(Some("3.2B")), Some(3_200));
        assert_eq!(parse_params(Some("qwen3.8:27b")), Some(27_000));
    }

    #[test]
    fn rerankers_are_not_used_for_chat() {
        let catalog = catalog(&[
            installed("dengcao/Qwen3-Reranker-0.6B:Q8_0", 639, 600),
            installed("llama3.2:3b", 2_000, 3_000),
        ]);
        assert_eq!(
            chat_model(AUTO, &prefs(None, None), &catalog, "hi", false, false),
            "llama3.2:3b"
        );
    }
}
