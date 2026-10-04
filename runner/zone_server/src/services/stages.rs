//! Optional per-stage model selection.
//!
//! Workspace/org settings and `OLLAMA_MODEL_*` / `COMFYUI_*` env vars can pin
//! every stage. When they are empty, chat messages pick from the models that
//! are actually installed. On a coding agent, a stage runs only a pin the
//! agent knows, and otherwise leaves the choice to the agent.

use reqwest::Client;
use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;
use zone_core::llm::{AgentKind, LlmBackend};

use crate::db::ai_settings::EffectiveAiSettings;
use crate::services::endpoint::{Endpoint, Origin};

#[cfg(test)]
pub(crate) mod testing;

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
    pub scope: Scope,
}

/// Which model names a completion may be sent under.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    /// Any name the catalog allows.
    #[default]
    Open,
    /// Only the Fast and Reasoning models the settings save. A chat or task
    /// may name a model of the instance's, which an endpoint the settings
    /// name does not serve.
    Saved,
}

/// What a stage runs when the saved settings name no model.
#[derive(Debug, Clone, Default)]
struct Fallbacks {
    fast: Option<String>,
    reasoning: Option<String>,
    embedding: Option<String>,
    vision: Option<String>,
    classifier: Option<String>,
}

impl Fallbacks {
    fn environment(classifier: &str) -> Self {
        Self {
            fast: env_optional("OLLAMA_MODEL_FAST"),
            reasoning: env_optional("OLLAMA_MODEL_REASON"),
            embedding: env_optional("OLLAMA_MODEL_EMBED"),
            vision: env_optional("OLLAMA_MODEL_VISION"),
            classifier: nonempty(Some(classifier))
                .or_else(|| env_optional("COMFYUI_CLASSIFIER_MODEL")),
        }
    }
}

impl Preferences {
    pub fn from_settings(settings: &EffectiveAiSettings, classifier: &str) -> Self {
        Self::layered(settings, Fallbacks::environment(classifier))
    }

    /// The preferences for completions sent to `endpoint`. The instance's
    /// `OLLAMA_MODEL_*` and classifier models name models on the instance's
    /// own endpoint, so an endpoint the settings name runs only the models the
    /// settings save.
    pub fn for_endpoint(
        settings: &EffectiveAiSettings,
        classifier: &str,
        endpoint: &Endpoint,
    ) -> Self {
        match endpoint.origin() {
            Origin::Instance => Self::from_settings(settings, classifier),
            Origin::Settings => Self {
                scope: Scope::Saved,
                ..Self::layered(settings, Fallbacks::default())
            },
        }
    }

    fn layered(settings: &EffectiveAiSettings, fallbacks: Fallbacks) -> Self {
        Self {
            fast: nonempty(settings.model_fast.as_deref()).or(fallbacks.fast),
            reasoning: nonempty(settings.model_reasoning.as_deref()).or(fallbacks.reasoning),
            embedding: nonempty(settings.model_embedding.as_deref()).or(fallbacks.embedding),
            vision: fallbacks.vision,
            classifier: nonempty(settings.model_fast.as_deref()).or(fallbacks.classifier),
            scope: Scope::Open,
        }
    }

    /// The Fast and Reasoning models, each named once.
    pub fn completions(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for name in [self.fast.as_deref(), self.reasoning.as_deref()]
            .into_iter()
            .flatten()
        {
            if !names.iter().any(|named| same_model(named, name)) {
                names.push(name);
            }
        }
        names
    }

    /// Whether a completion may be sent under `name`.
    pub fn admits(&self, name: &str) -> bool {
        match self.scope {
            Scope::Open => true,
            Scope::Saved => [self.fast.as_deref(), self.reasoning.as_deref()]
                .into_iter()
                .flatten()
                .any(|saved| same_model(saved, name.trim())),
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
    /// Whether the model can call tools, or `None` when the endpoint did not
    /// say, as Ollama before capabilities and some proxies do not.
    pub tools: Option<bool>,
}

impl Installed {
    pub fn completion(&self) -> bool {
        !self.embedding && !self.reranker
    }

    pub fn refuses_tools(&self) -> bool {
        self.tools == Some(false)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub models: Vec<Installed>,
    /// The coding agent that completes instead of the endpoint. It runs the
    /// models it knows, whatever Ollama has installed.
    pub agent: Option<AgentKind>,
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
            agent: None,
        }
    }

    /// The models an agent offers, in its own order.
    pub fn agent(agent: AgentKind) -> Self {
        Self {
            models: agent
                .models()
                .iter()
                .map(|name| Installed {
                    name: (*name).to_string(),
                    bytes: 0,
                    million_params: None,
                    embedding: false,
                    vision: false,
                    reranker: false,
                    tools: Some(true),
                })
                .collect(),
            agent: Some(agent),
        }
    }

    /// What `backend` can run: Ollama's installed models behind the
    /// endpoint, or the agent's own.
    pub async fn for_backend(ollama_host: &str, backend: &LlmBackend) -> Self {
        match backend {
            LlmBackend::Http => Self::load(ollama_host).await,
            LlmBackend::Cli { agent, .. } => Self::agent(*agent),
        }
    }

    /// Whether a model named in settings runs under that name. The endpoint
    /// routes whatever it is given; an agent is given only names it knows,
    /// and runs a model of its own choosing in place of any other.
    pub fn accepts(&self, name: &str) -> bool {
        self.agent.is_none_or(|agent| agent.knows(name))
    }

    /// Whether a completion left on [`AUTO`] still runs: an agent chooses its
    /// own model, while the endpoint has to be given one.
    pub fn chooses(&self) -> bool {
        self.agent.is_some()
    }

    pub fn contains(&self, name: &str) -> bool {
        match self.agent {
            Some(agent) => agent.knows(name),
            None => self.find(name).is_some(),
        }
    }

    pub fn find(&self, name: &str) -> Option<&Installed> {
        self.models
            .iter()
            .find(|model| same_model(&model.name, name))
    }

    /// Whether `name` is listed as a model that cannot call tools. A name the
    /// catalog does not list, or lists without its capabilities, may.
    pub fn refuses_tools(&self, name: &str) -> bool {
        self.find(name).is_some_and(Installed::refuses_tools)
    }

    /// Every installed model that completes chats, in catalog order.
    pub(crate) fn completions(&self) -> impl Iterator<Item = &Installed> {
        self.models.iter().filter(|model| model.completion())
    }

    /// The completion models Zone may put a run on without anyone naming
    /// one: all of them, less those the agent runs only when named.
    pub(crate) fn unattended(&self) -> impl Iterator<Item = &Installed> {
        self.completions().filter(|model| {
            self.agent
                .is_none_or(|agent| !agent.named_only().contains(&model.name.as_str()))
        })
    }
}

/// True when the chat should pick a stage from the message instead of a pin.
pub fn is_auto(name: &str) -> bool {
    let name = name.trim();
    name.is_empty() || name.eq_ignore_ascii_case(AUTO)
}

/// Chat completion model for this message.
///
/// On an agent's catalog a requested name the agent does not know counts as
/// [`AUTO`], and so does a preference it does not know; [`AUTO`] then means
/// the agent chooses. A requested name the preferences do not admit counts
/// as [`AUTO`] too.
pub fn chat_model(
    requested: &str,
    preferences: &Preferences,
    catalog: &Catalog,
    message: &str,
    has_image: bool,
    agent: bool,
) -> String {
    if let Some(kind) = catalog.agent {
        let preferred = if wants_reason(message) {
            &preferences.reasoning
        } else {
            &preferences.fast
        };
        return known(kind, [Some(requested), preferred.as_deref()]);
    }
    let requested = if preferences.admits(requested) {
        requested
    } else {
        AUTO
    };
    if !is_auto(requested) {
        return requested.to_string();
    }
    let tools = if agent {
        Tools::Required
    } else {
        Tools::Optional
    };
    if has_image
        && let Some(model) =
            pick_installed(preferences.vision.as_deref(), catalog, Stage::Vision, tools)
    {
        return model;
    }
    if wants_reason(message)
        && let Some(model) = pick_installed(
            preferences.reasoning.as_deref(),
            catalog,
            Stage::Reason,
            tools,
        )
    {
        return model;
    }
    pick_installed(preferences.fast.as_deref(), catalog, Stage::Fast, tools)
        .or_else(|| {
            pick_installed(
                preferences.reasoning.as_deref(),
                catalog,
                Stage::Reason,
                tools,
            )
        })
        .unwrap_or_else(|| {
            fallback_name(
                requested,
                preferences
                    .fast
                    .as_deref()
                    .filter(|name| tools.allows(catalog, name)),
            )
        })
}

/// The model that classifies image intent, and that [`summary_model`] runs.
///
/// On an agent's catalog, only a classifier or fast model the agent knows;
/// otherwise [`AUTO`].
pub fn classifier_model(preferences: &Preferences, catalog: &Catalog, chat_model: &str) -> String {
    if let Some(kind) = catalog.agent {
        return known(
            kind,
            [
                preferences.classifier.as_deref(),
                preferences.fast.as_deref(),
            ],
        );
    }
    if let Some(name) = preferences
        .classifier
        .as_deref()
        .filter(|name| !is_auto(name))
        && catalog_allows(catalog, name)
    {
        return name.to_string();
    }
    if !is_auto(chat_model) && preferences.admits(chat_model) && catalog_allows(catalog, chat_model)
    {
        return chat_model.to_string();
    }
    pick_installed(
        preferences.fast.as_deref(),
        catalog,
        Stage::Fast,
        Tools::Optional,
    )
    .or_else(|| {
        catalog
            .completions()
            .min_by_key(|model| (model.million_params.unwrap_or(u32::MAX), model.bytes))
            .map(|model| model.name.clone())
    })
    .unwrap_or_else(|| fallback_name(AUTO, preferences.classifier.as_deref()))
}

/// The model a chat title, a pull request subject or a merge summary runs on,
/// or `None` when there is nothing to run it on: an agent chooses its own on
/// [`AUTO`], and the endpoint has to be given a name.
pub fn summary_model(
    preferences: &Preferences,
    catalog: &Catalog,
    chat_model: &str,
) -> Option<String> {
    let model = classifier_model(preferences, catalog, chat_model);
    (catalog.chooses() || !is_auto(&model)).then_some(model)
}

/// The first of `names` the agent knows, or [`AUTO`] for it to choose.
fn known<'a>(kind: AgentKind, names: impl IntoIterator<Item = Option<&'a str>>) -> String {
    names
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|name| kind.knows(name))
        .unwrap_or(AUTO)
        .to_string()
}

#[derive(Clone, Copy)]
enum Stage {
    Fast,
    Reason,
    Vision,
}

/// Whether the completion will offer tools, as an agent run does.
#[derive(Clone, Copy)]
enum Tools {
    Required,
    Optional,
}

impl Tools {
    fn admits(self, model: &Installed) -> bool {
        match self {
            Self::Required => !model.refuses_tools(),
            Self::Optional => true,
        }
    }

    /// A name the catalog does not list may call tools.
    fn allows(self, catalog: &Catalog, name: &str) -> bool {
        catalog.find(name).is_none_or(|model| self.admits(model))
    }
}

fn pick_installed(
    preferred: Option<&str>,
    catalog: &Catalog,
    stage: Stage,
    tools: Tools,
) -> Option<String> {
    if let Some(name) = preferred.filter(|name| !is_auto(name))
        && catalog_allows(catalog, name)
        && tools.allows(catalog, name)
    {
        return Some(name.to_string());
    }
    if catalog.models.is_empty() {
        return preferred.filter(|name| !is_auto(name)).map(str::to_string);
    }
    let mut candidates: Vec<&Installed> = catalog
        .completions()
        .filter(|model| tools.admits(model))
        .filter(|model| match stage {
            Stage::Vision => model.vision,
            Stage::Fast | Stage::Reason => true,
        })
        .collect();
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

/// Whether two model names refer to one model, compared the way the catalog compares them.
pub(crate) fn same_model(left: &str, right: &str) -> bool {
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
        let has = |capability: Capability| {
            tag.capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.contains(&capability))
        };
        Self {
            million_params: tag
                .details
                .as_ref()
                .and_then(|details| parse_params(details.parameter_size.as_deref()))
                .or_else(|| parse_params(Some(&tag.name))),
            embedding: has(Capability::Embedding) || lower.contains("embed"),
            vision: has(Capability::Vision)
                || lower.contains("llava")
                || lower.contains("vision")
                || lower.contains("minicpm-v"),
            reranker: lower.contains("rerank"),
            tools: tag.capabilities.as_ref().map(|_| has(Capability::Tools)),
            name: tag.name,
            bytes: tag.size,
        }
    }
}

pub(crate) fn parse_params(value: Option<&str>) -> Option<u32> {
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
    capabilities: Option<Vec<Capability>>,
}

#[derive(Deserialize)]
struct TagDetails {
    parameter_size: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Capability {
    Completion,
    Tools,
    Vision,
    Embedding,
    Thinking,
    Insert,
    #[serde(other)]
    Unknown,
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
            tools: None,
        }
    }

    fn catalog(models: &[Installed]) -> Catalog {
        Catalog {
            models: models.to_vec(),
            agent: None,
        }
    }

    fn preferences(fast: Option<&str>, reason: Option<&str>) -> Preferences {
        Preferences {
            fast: fast.map(str::to_string),
            reasoning: reason.map(str::to_string),
            embedding: None,
            vision: Some("llava:7b".to_string()),
            classifier: fast.map(str::to_string),
            scope: Scope::Open,
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
            chat_model(
                AUTO,
                &preferences(None, None),
                &catalog,
                "hello",
                false,
                false
            ),
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
                &preferences(Some("qwen3.8:27b"), None),
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
                &preferences(Some("llama3.1:8b"), Some("llama3.1:8b")),
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
                &preferences(Some("llama3.2:3b"), None),
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
                &preferences(None, None),
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
            classifier_model(&preferences(None, None), &catalog, "qwen3.8:27b"),
            "qwen3.8:27b"
        );
    }

    #[test]
    fn classifier_skips_uninstalled_env_pin() {
        let catalog = catalog(&[installed("llama3.2:3b", 2_000, 3_000)]);
        let preferences = Preferences {
            fast: None,
            reasoning: None,
            embedding: None,
            vision: None,
            classifier: Some("llama3.1:8b".to_string()),
            scope: Scope::Open,
        };
        assert_eq!(
            classifier_model(&preferences, &catalog, AUTO),
            "llama3.2:3b"
        );
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
                &preferences(None, None),
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
            chat_model(AUTO, &preferences(None, None), &catalog, "hi", false, false),
            "llama3.2:3b"
        );
    }

    const REASONING: &str = "Prove that this algorithm is correct and list edge cases";

    fn classifying(classifier: Option<&str>, fast: Option<&str>) -> Preferences {
        Preferences {
            fast: fast.map(str::to_string),
            classifier: classifier.map(str::to_string),
            ..Preferences::default()
        }
    }

    fn names(catalog: &Catalog) -> Vec<&str> {
        catalog
            .models
            .iter()
            .map(|model| model.name.as_str())
            .collect()
    }

    #[test]
    fn an_agent_runs_a_requested_model_it_knows() {
        let claude = Catalog::agent(AgentKind::Claude);
        let preferences = preferences(Some("haiku"), Some("sonnet"));

        assert_eq!(
            chat_model("opus", &preferences, &claude, "hello", false, true),
            "opus"
        );
        assert_eq!(
            chat_model(
                "claude-opus-4-1",
                &preferences,
                &claude,
                REASONING,
                false,
                true
            ),
            "claude-opus-4-1",
            "a full name the agent knows is kept, though the agent offers only aliases"
        );
    }

    #[test]
    fn a_requested_model_the_agent_does_not_know_is_left_to_the_preferences() {
        let claude = Catalog::agent(AgentKind::Claude);
        let preferences = preferences(Some("haiku"), Some("opus"));

        assert_eq!(
            chat_model("llama3.2:3b", &preferences, &claude, "hello", false, true),
            "haiku"
        );
        assert_eq!(
            chat_model("llama3.2:3b", &preferences, &claude, REASONING, false, true),
            "opus"
        );
    }

    #[test]
    fn an_agent_reasons_on_the_reasoning_model_and_answers_on_the_fast_one() {
        let codex = Catalog::agent(AgentKind::Codex);
        let preferences = preferences(Some("gpt-6-luna"), Some("gpt-6-astra"));

        assert_eq!(
            chat_model(AUTO, &preferences, &codex, "hello", false, true),
            "gpt-6-luna"
        );
        assert_eq!(
            chat_model(AUTO, &preferences, &codex, REASONING, false, true),
            "gpt-6-astra"
        );
    }

    #[test]
    fn an_agent_chooses_its_own_model_when_no_preference_is_one_it_knows() {
        let claude = Catalog::agent(AgentKind::Claude);
        let installed = preferences(Some("llama3.2:3b"), Some("qwen3.8:27b"));

        assert_eq!(
            chat_model(AUTO, &installed, &claude, "hello", false, true),
            AUTO
        );
        assert_eq!(
            chat_model(AUTO, &installed, &claude, REASONING, true, true),
            AUTO
        );
        assert_eq!(
            chat_model(
                AUTO,
                &preferences(None, None),
                &claude,
                "hello",
                false,
                false
            ),
            AUTO
        );
        assert_eq!(
            chat_model(
                AUTO,
                &preferences(Some("gpt-6-sol"), None),
                &claude,
                "hello",
                false,
                true
            ),
            AUTO,
            "another agent's model is not one this agent runs"
        );
    }

    #[test]
    fn each_prompt_uses_only_its_own_stage_before_the_agent_chooses() {
        let claude = Catalog::agent(AgentKind::Claude);

        assert_eq!(
            chat_model(
                AUTO,
                &preferences(Some("haiku"), None),
                &claude,
                REASONING,
                false,
                true
            ),
            AUTO,
            "a reasoning prompt is not handed to the fast model"
        );
        assert_eq!(
            chat_model(
                AUTO,
                &preferences(None, Some("opus")),
                &claude,
                "hello",
                false,
                true
            ),
            AUTO
        );
    }

    #[test]
    fn an_agent_classifies_on_a_classifier_or_fast_model_it_knows_or_not_at_all() {
        let claude = Catalog::agent(AgentKind::Claude);

        assert_eq!(
            classifier_model(&classifying(Some("haiku"), Some("sonnet")), &claude, AUTO),
            "haiku"
        );
        assert_eq!(
            classifier_model(
                &classifying(Some("llama3.2:3b"), Some("sonnet")),
                &claude,
                AUTO
            ),
            "sonnet"
        );
        assert_eq!(
            classifier_model(&classifying(Some("llama3.2:3b"), None), &claude, "opus"),
            AUTO,
            "the chat's own model is not borrowed to classify"
        );
    }

    #[test]
    fn a_summary_runs_on_the_agents_choice_but_never_on_the_endpoints_auto() {
        let claude = Catalog::agent(AgentKind::Claude);
        let unknown = classifying(Some("llama3.2:3b"), Some("llama3.2:3b"));

        assert_eq!(
            summary_model(&unknown, &claude, AUTO).as_deref(),
            Some(AUTO)
        );
        assert_eq!(
            summary_model(&classifying(None, Some("haiku")), &claude, AUTO).as_deref(),
            Some("haiku")
        );
        assert_eq!(
            summary_model(&Preferences::default(), &catalog(&[]), AUTO),
            None,
            "the endpoint was handed auto with nothing installed to route it to"
        );
        assert_eq!(
            summary_model(
                &Preferences::default(),
                &catalog(&[installed("llama3.2:3b", 2_000, 3_000)]),
                AUTO
            )
            .as_deref(),
            Some("llama3.2:3b")
        );
    }

    #[test]
    fn an_agent_contains_every_model_it_knows_and_nothing_installed() {
        let claude = Catalog::agent(AgentKind::Claude);
        let codex = Catalog::agent(AgentKind::Codex);

        assert!(claude.contains("sonnet"));
        assert!(claude.contains("claude-opus-4-1"));
        assert!(!claude.contains("llama3.2:3b"));
        assert!(!claude.contains(AUTO));
        assert!(codex.contains("gpt-6-sol"));
        assert!(!codex.contains("sonnet"));
    }

    #[test]
    fn only_an_agent_refuses_a_name_or_runs_without_one() {
        let installed = catalog(&[installed("llama3.2:3b", 2_000, 3_000)]);
        let claude = Catalog::agent(AgentKind::Claude);

        assert!(
            installed.accepts("gpt-4o"),
            "the endpoint routes names Ollama does not have"
        );
        assert!(!installed.chooses());
        assert!(claude.accepts("opus"));
        assert!(!claude.accepts("llama3.2:3b"));
        assert!(claude.chooses());
    }

    #[test]
    fn tags_report_what_each_model_can_do_and_a_tag_without_capabilities_is_left_unknown() {
        let tags: Tags = serde_json::from_value(serde_json::json!({"models": [
            {
                "name": "llava:7b",
                "size": 4_733_363_377_u64,
                "details": {"parameter_size": "7B"},
                "capabilities": ["completion", "vision"]
            },
            {
                "name": "qwen3.8:27b",
                "size": 17_741_872_154_u64,
                "details": {"parameter_size": "27.3B"},
                "capabilities": ["completion", "tools", "thinking", "vision"]
            },
            {
                "name": "hf.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF:Q4_K_M",
                "size": 491_413_571,
                "capabilities": ["completion", "tools", "insert", "audio"]
            },
            {
                "name": "qwen3-embedding:0.6b",
                "size": 639_150_858,
                "capabilities": ["embedding"]
            },
            {"name": "llama3.2:3b", "size": 2_019_393_189_u64}
        ]}))
        .expect("the tags Ollama lists");
        let catalog = Catalog {
            models: tags.models.into_iter().map(Installed::from).collect(),
            agent: None,
        };
        let model = |name: &str| catalog.find(name).expect(name).clone();

        let llava = model("llava:7b");
        assert_eq!(llava.tools, Some(false));
        assert!(llava.vision);
        let qwen = model("qwen3.8:27b");
        assert_eq!(qwen.tools, Some(true));
        assert!(
            qwen.vision,
            "vision comes from the capabilities, not the name"
        );
        assert_eq!(
            model("hf.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF:Q4_K_M").tools,
            Some(true),
            "a capability Zone does not know is ignored"
        );
        assert!(model("qwen3-embedding:0.6b").embedding);
        assert_eq!(model("llama3.2:3b").tools, None);

        assert!(catalog.refuses_tools("llava:7b"));
        assert!(!catalog.refuses_tools("qwen3.8:27b"));
        assert!(!catalog.refuses_tools("llama3.2:3b"), "unknown is allowed");
        assert!(
            !catalog.refuses_tools("gpt-4o"),
            "an unlisted name is allowed"
        );
    }

    #[test]
    fn every_model_an_agent_offers_can_call_tools() {
        let claude = Catalog::agent(AgentKind::Claude);

        assert!(claude.models.iter().all(|model| model.tools == Some(true)));
    }

    #[tokio::test]
    async fn an_agent_backend_runs_its_own_models_whatever_ollama_has_installed() {
        use serde_json::json;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        use zone_core::llm::CliSettings;

        let ollama = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"models": [{"name": "llama3.2:3b", "size": 2_000}]})),
            )
            .mount(&ollama)
            .await;

        let codex = Catalog::for_backend(
            &ollama.uri(),
            &LlmBackend::cli(AgentKind::Codex, CliSettings::default()),
        )
        .await;
        let endpoint = Catalog::for_backend(&ollama.uri(), &LlmBackend::Http).await;

        assert_eq!(codex.agent, Some(AgentKind::Codex));
        assert_eq!(names(&codex), AgentKind::Codex.models());
        assert_eq!(endpoint.agent, None);
        assert_eq!(names(&endpoint), ["llama3.2:3b"]);
        assert_eq!(
            ollama
                .received_requests()
                .await
                .map(|requests| requests.len()),
            Some(1),
            "only the endpoint's catalog is read from Ollama"
        );
    }

    fn vision(model: Installed, tools: Option<bool>) -> Installed {
        Installed {
            vision: true,
            tools,
            ..model
        }
    }

    #[test]
    fn an_embedding_model_that_reads_images_is_never_the_vision_pick() {
        let catalog = catalog(&[
            Installed {
                embedding: true,
                vision: true,
                ..installed("nomic-embed-vision:v1.5", 262, 93)
            },
            installed("llava:7b", 4_733, 7_000),
        ]);

        assert_eq!(
            chat_model(
                AUTO,
                &Preferences::default(),
                &catalog,
                "what is this",
                true,
                false
            ),
            "llava:7b"
        );
    }

    #[test]
    fn an_image_chat_takes_the_smallest_model_that_reads_images_whether_dedicated_or_general() {
        let llava = vision(installed("llava:7b", 4_733, 7_000), Some(false));
        let qwen = vision(installed("qwen3.8:27b", 17_741, 27_300), Some(true));
        let gemma = vision(installed("gemma3:4b", 3_338, 4_300), Some(false));
        let image = |models: &[Installed]| {
            chat_model(
                AUTO,
                &Preferences::default(),
                &catalog(models),
                "what is this",
                true,
                false,
            )
        };

        assert_eq!(
            image(&[qwen.clone(), llava.clone()]),
            "llava:7b",
            "a general model that reads images does not displace a smaller dedicated one"
        );
        assert_eq!(
            image(&[qwen.clone(), installed("llama3.2:3b", 2_019, 3_200)]),
            "qwen3.8:27b",
            "the only model that reads images beats a smaller one that cannot"
        );
        assert_eq!(
            image(&[installed("llava:13b", 8_000, 13_000), gemma]),
            "gemma3:4b",
            "a smaller general model beats a larger dedicated one"
        );
    }

    #[test]
    fn an_agent_run_never_lands_on_a_model_that_refuses_tools() {
        let llava = vision(installed("llava:7b", 4_733, 7_000), Some(false));
        let qwen = vision(installed("qwen3.8:27b", 17_741, 27_300), Some(true));
        let catalog = catalog(&[llava, qwen]);
        let task = |preferences: &Preferences, message: &str, agent: bool| {
            chat_model(AUTO, preferences, &catalog, message, false, agent)
        };

        assert_eq!(
            task(&Preferences::default(), "Add a cart", true),
            "qwen3.8:27b"
        );
        assert_eq!(
            task(&Preferences::default(), "Audit the cart", true),
            "qwen3.8:27b"
        );
        assert_eq!(
            task(
                &preferences(Some("llava:7b"), Some("llava:7b")),
                "Add a cart",
                true
            ),
            "qwen3.8:27b",
            "a preference that cannot call tools is passed over for a run that offers them"
        );
        assert_eq!(
            task(&Preferences::default(), "Add a cart", false),
            "llava:7b",
            "a plain chat offers no tools, so the smallest model still answers it"
        );
    }

    #[test]
    fn an_agent_run_keeps_a_model_whose_tools_are_unknown_and_names_none_that_refuses_them() {
        let unknown = catalog(&[installed("llama3.2:3b", 2_019, 3_200)]);
        let refusing = catalog(&[vision(installed("llava:7b", 4_733, 7_000), Some(false))]);

        assert_eq!(
            chat_model(
                AUTO,
                &Preferences::default(),
                &unknown,
                "Add a cart",
                false,
                true
            ),
            "llama3.2:3b"
        );
        assert_eq!(
            chat_model(
                AUTO,
                &preferences(Some("llava:7b"), None),
                &refusing,
                "Add a cart",
                false,
                true
            ),
            AUTO
        );
    }

    fn openai(fast: Option<&str>, reasoning: Option<&str>) -> EffectiveAiSettings {
        let mut settings = crate::services::endpoint::testing::settings(
            zone_context::embeddings::providers::PROVIDER_OPENAI,
        );
        settings.openai_api_key = Some(abnegate_secret::SecretValue::new("sk-organization-key"));
        settings.model_fast = fast.map(str::to_string);
        settings.model_reasoning = reasoning.map(str::to_string);
        settings
    }

    fn instance_models() -> Fallbacks {
        Fallbacks {
            fast: Some("qwen2.5:7b-instruct".to_string()),
            reasoning: Some("deepseek-r1:14b".to_string()),
            embedding: Some("nomic-embed-text".to_string()),
            vision: Some("llava:7b".to_string()),
            classifier: Some("qwen2.5:3b".to_string()),
        }
    }

    #[test]
    fn unsaved_stages_fall_back_to_the_instances_models() {
        let preferences = Preferences::layered(&openai(None, None), instance_models());

        assert_eq!(preferences.fast.as_deref(), Some("qwen2.5:7b-instruct"));
        assert_eq!(preferences.reasoning.as_deref(), Some("deepseek-r1:14b"));
        assert_eq!(preferences.embedding.as_deref(), Some("nomic-embed-text"));
        assert_eq!(preferences.vision.as_deref(), Some("llava:7b"));
        assert_eq!(preferences.classifier.as_deref(), Some("qwen2.5:3b"));
    }

    #[test]
    fn an_endpoint_the_settings_name_runs_no_model_of_the_instances() {
        let settings = openai(None, None);
        let endpoint =
            crate::services::endpoint::Endpoint::resolve(&crate::state::test_config(), &settings);
        assert_eq!(endpoint.origin(), Origin::Settings);

        let preferences = Preferences::for_endpoint(&settings, "qwen2.5:3b", &endpoint);

        assert_eq!(preferences.fast, None);
        assert_eq!(preferences.reasoning, None);
        assert_eq!(preferences.embedding, None);
        assert_eq!(preferences.vision, None);
        assert_eq!(
            preferences.classifier, None,
            "the instance's classifier is no model here"
        );
        let chosen = chat_model(
            AUTO,
            &preferences,
            &Catalog::default(),
            "hello",
            false,
            false,
        );
        assert_eq!(
            endpoint.model(&chosen),
            Err(crate::services::endpoint::Error::ModelUnset),
            "an unset model is refused before a provider is asked for {chosen}"
        );
    }

    #[test]
    fn an_endpoint_the_settings_name_runs_the_models_they_save() {
        let settings = openai(Some("gpt-4o-mini"), Some("o3"));
        let endpoint =
            crate::services::endpoint::Endpoint::resolve(&crate::state::test_config(), &settings);

        let preferences = Preferences::for_endpoint(&settings, "qwen2.5:3b", &endpoint);

        assert_eq!(preferences.fast.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(preferences.reasoning.as_deref(), Some("o3"));
        assert_eq!(preferences.classifier.as_deref(), Some("gpt-4o-mini"));
        let catalog = Catalog::default();
        assert_eq!(
            chat_model(AUTO, &preferences, &catalog, "hello", false, false),
            "gpt-4o-mini"
        );
        assert_eq!(
            chat_model(
                AUTO,
                &preferences,
                &catalog,
                "find the root cause",
                false,
                false
            ),
            "o3"
        );
        assert_eq!(
            summary_model(&preferences, &catalog, AUTO).as_deref(),
            Some("gpt-4o-mini")
        );
    }

    #[test]
    fn the_instances_endpoint_keeps_the_instances_classifier() {
        let settings = crate::services::endpoint::testing::settings(
            zone_context::embeddings::providers::PROVIDER_SELF_HOSTED,
        );
        let endpoint = crate::services::endpoint::Endpoint::instance(&crate::state::test_config());

        let preferences = Preferences::for_endpoint(&settings, "qwen2.5:3b", &endpoint);

        assert_eq!(preferences.classifier.as_deref(), Some("qwen2.5:3b"));
    }

    fn saved_endpoint(settings: &EffectiveAiSettings) -> Preferences {
        let endpoint =
            crate::services::endpoint::Endpoint::resolve(&crate::state::test_config(), settings);
        assert_eq!(endpoint.origin(), Origin::Settings);
        Preferences::for_endpoint(settings, "qwen2.5:3b", &endpoint)
    }

    #[test]
    fn an_endpoint_the_settings_name_is_never_sent_a_model_they_do_not_save() {
        let preferences = saved_endpoint(&openai(Some("gpt-4o-mini"), Some("o3")));
        let catalog = Catalog::default();

        assert_eq!(
            chat_model("llama3.2:3b", &preferences, &catalog, "hello", false, false),
            "gpt-4o-mini",
            "a chat pinned to an instance model was sent it on the saved endpoint"
        );
        assert_eq!(
            chat_model(
                "llama3.2:3b",
                &preferences,
                &catalog,
                "find the root cause",
                false,
                true
            ),
            "o3"
        );
        assert_eq!(
            chat_model("o3", &preferences, &catalog, "hello", false, false),
            "o3",
            "a saved model the chat names is kept"
        );
        assert_eq!(
            summary_model(&preferences, &catalog, "llama3.2:3b").as_deref(),
            Some("gpt-4o-mini")
        );

        let unsaved = saved_endpoint(&openai(None, None));
        let chosen = chat_model("llama3.2:3b", &unsaved, &catalog, "hello", false, false);
        assert_eq!(chosen, AUTO, "nothing saved leaves nothing to send");
        assert_eq!(summary_model(&unsaved, &catalog, "llama3.2:3b"), None);

        let reasoning = saved_endpoint(&openai(None, Some("o3")));
        assert_eq!(
            summary_model(&reasoning, &catalog, "llama3.2:3b"),
            None,
            "a title was asked of an instance model on the saved endpoint"
        );
    }

    #[test]
    fn a_saved_endpoint_offers_its_fast_and_reasoning_models_once_each() {
        assert_eq!(
            saved_endpoint(&openai(Some("gpt-4o-mini"), Some("o3"))).completions(),
            ["gpt-4o-mini", "o3"]
        );
        assert_eq!(
            saved_endpoint(&openai(Some("o3"), Some("o3:latest"))).completions(),
            ["o3"]
        );
        assert!(saved_endpoint(&openai(None, None)).completions().is_empty());
    }

    #[test]
    fn the_instances_endpoint_runs_any_model_a_chat_names() {
        let preferences = Preferences::for_endpoint(
            &openai(None, None),
            "qwen2.5:3b",
            &crate::services::endpoint::Endpoint::instance(&crate::state::test_config()),
        );

        assert_eq!(preferences.scope, Scope::Open);
        assert_eq!(
            chat_model(
                "llama3.2:3b",
                &preferences,
                &Catalog::default(),
                "hello",
                false,
                false
            ),
            "llama3.2:3b"
        );
    }

    #[test]
    fn a_saved_model_a_chat_names_runs_its_summaries_on_an_endpoint_that_lists_nothing() {
        let preferences = saved_endpoint(&openai(None, Some("o3")));

        assert_eq!(
            summary_model(&preferences, &Catalog::default(), "o3").as_deref(),
            Some("o3"),
            "titles and summaries stopped because the endpoint lists no models"
        );
        assert_eq!(
            summary_model(&Preferences::default(), &Catalog::default(), "llama3.2:3b").as_deref(),
            Some("llama3.2:3b"),
            "an instance whose catalog cannot be read still summarizes on the chat's model"
        );
    }
}
