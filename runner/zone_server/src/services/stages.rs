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

    /// Every installed model that completes chats, in catalog order.
    pub(crate) fn completions(&self) -> impl Iterator<Item = &Installed> {
        self.models.iter().filter(|model| model.completion())
    }

    /// The completion models Zone may put a run on without anyone naming
    /// one: all of them, less those the agent gates behind consent.
    pub(crate) fn unattended(&self) -> impl Iterator<Item = &Installed> {
        self.completions().filter(|model| {
            self.agent
                .is_none_or(|agent| !agent.gated().contains(&model.name.as_str()))
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
/// the agent chooses.
pub fn chat_model(
    requested: &str,
    prefs: &Preferences,
    catalog: &Catalog,
    message: &str,
    has_image: bool,
    agent: bool,
) -> String {
    if let Some(kind) = catalog.agent {
        let preferred = if wants_reason(message) {
            &prefs.reasoning
        } else {
            &prefs.fast
        };
        return known(kind, [Some(requested), preferred.as_deref()]);
    }
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

/// The model that classifies image intent, and that [`summary_model`] runs.
///
/// On an agent's catalog, only a classifier or fast model the agent knows;
/// otherwise [`AUTO`].
pub fn classifier_model(prefs: &Preferences, catalog: &Catalog, chat_model: &str) -> String {
    if let Some(kind) = catalog.agent {
        return known(kind, [prefs.classifier.as_deref(), prefs.fast.as_deref()]);
    }
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

/// The model a chat title, a pull request subject or a merge summary runs on,
/// or `None` when there is nothing to run it on: an agent chooses its own on
/// [`AUTO`], and the endpoint has to be given a name.
pub fn summary_model(prefs: &Preferences, catalog: &Catalog, chat_model: &str) -> Option<String> {
    let model = classifier_model(prefs, catalog, chat_model);
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
            agent: None,
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
        let prefs = prefs(Some("haiku"), Some("sonnet"));

        assert_eq!(
            chat_model("opus", &prefs, &claude, "hello", false, true),
            "opus"
        );
        assert_eq!(
            chat_model("claude-opus-4-1", &prefs, &claude, REASONING, false, true),
            "claude-opus-4-1",
            "a full name the agent knows is kept, though the agent offers only aliases"
        );
    }

    #[test]
    fn a_requested_model_the_agent_does_not_know_is_left_to_the_preferences() {
        let claude = Catalog::agent(AgentKind::Claude);
        let prefs = prefs(Some("haiku"), Some("opus"));

        assert_eq!(
            chat_model("llama3.2:3b", &prefs, &claude, "hello", false, true),
            "haiku"
        );
        assert_eq!(
            chat_model("llama3.2:3b", &prefs, &claude, REASONING, false, true),
            "opus"
        );
    }

    #[test]
    fn an_agent_reasons_on_the_reasoning_model_and_answers_on_the_fast_one() {
        let codex = Catalog::agent(AgentKind::Codex);
        let prefs = prefs(Some("gpt-6-luna"), Some("gpt-6-astra"));

        assert_eq!(
            chat_model(AUTO, &prefs, &codex, "hello", false, true),
            "gpt-6-luna"
        );
        assert_eq!(
            chat_model(AUTO, &prefs, &codex, REASONING, false, true),
            "gpt-6-astra"
        );
    }

    #[test]
    fn an_agent_chooses_its_own_model_when_no_preference_is_one_it_knows() {
        let claude = Catalog::agent(AgentKind::Claude);
        let installed = prefs(Some("llama3.2:3b"), Some("qwen3.8:27b"));

        assert_eq!(
            chat_model(AUTO, &installed, &claude, "hello", false, true),
            AUTO
        );
        assert_eq!(
            chat_model(AUTO, &installed, &claude, REASONING, true, true),
            AUTO
        );
        assert_eq!(
            chat_model(AUTO, &prefs(None, None), &claude, "hello", false, false),
            AUTO
        );
        assert_eq!(
            chat_model(
                AUTO,
                &prefs(Some("gpt-6-sol"), None),
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
                &prefs(Some("haiku"), None),
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
                &prefs(None, Some("opus")),
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
}
