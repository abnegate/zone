//! Shared preparation, projection, and durable generation boundaries for both chat modes.

mod generation;
mod pinned;
pub mod prompt;
pub mod tail;

pub use generation::Generation;
pub use pinned::Pinned;

use abnegate_agent::chat::{self, Capacity, History, NewEntry, ReplayMessage};
use abnegate_agent::context::{
    self, ContextSource, ContextUsage, Coverage, Entry, Policy, Summary,
};
use abnegate_llm::{Message, ReasoningEffort, Role};
use abnegate_search::SearchContext;
use base64::Engine;
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;
use zone_core::llm::LlmClient;

use crate::agent::learned;
use crate::agent::prompt::Environment;
use crate::agent::{ChatTools, LoopBudget, WorkspaceScope};
use crate::db::chats::ChatRow;
use crate::db::context::{Error, Guard, Lease, Store};
use crate::db::knowledge::not_memory;
use crate::db::tasks::Origin;
use crate::services::artifacts::ArtifactStore;
use crate::services::backend;
use crate::services::chat::driver::Driver;
use crate::services::chat::handover::Handover;
use crate::services::completion_tokens::merge_stops;
use crate::services::endpoint::Endpoint;
use crate::services::project_directory::{self, Start};
use crate::state::AppState;

pub const LEASE_LIFETIME: Duration = Duration::from_secs(30);
/// The id of the system entry a turn's context opens with.
pub const INSTRUCTIONS: &str = "instructions";
const TEMPERATURE: f32 = 0.7;
const SEARCH: &str = "supplement:search";
pub const WITHHELD_IMAGE: &str = "[An image is omitted here because this model can't view images.]";

fn parse_timeout(seconds: u64) -> Result<Duration, String> {
    let timeout = Duration::from_secs(seconds);
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| "ZONE_CHAT_TIMEOUT_SECONDS exceeds a representable deadline".to_string())?;
    Ok(timeout)
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub rounds: usize,
    pub calls: usize,
    pub output: u32,
    pub context: u64,
    pub timeout: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rounds: 64,
            calls: 256,
            output: 4096,
            context: 32768,
            timeout: Duration::from_secs(1800),
        }
    }
}

impl Settings {
    pub fn from_env() -> Result<Self, String> {
        fn value(name: &str, default: u64) -> Result<u64, String> {
            match std::env::var(name) {
                Ok(value) => value
                    .parse::<u64>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| format!("{name} must be a positive integer")),
                Err(std::env::VarError::NotPresent) => Ok(default),
                Err(_) => Err(format!("{name} must contain valid text")),
            }
        }
        Ok(Self {
            context: value("ZONE_CHAT_CONTEXT_TOKENS", 32768)?,
            timeout: parse_timeout(value("ZONE_CHAT_TIMEOUT_SECONDS", 1800)?)?,
            rounds: usize::try_from(value("ZONE_CHAT_ROUNDS", 64)?)
                .map_err(|_| "ZONE_CHAT_ROUNDS exceeds platform range")?,
            calls: usize::try_from(value("ZONE_CHAT_CALLS", 256)?)
                .map_err(|_| "ZONE_CHAT_CALLS exceeds platform range")?,
            output: u32::try_from(value("ZONE_CHAT_OUTPUT_TOKENS", 4096)?)
                .map_err(|_| "ZONE_CHAT_OUTPUT_TOKENS exceeds provider range")?,
        })
    }

    pub fn budget(&self) -> LoopBudget {
        LoopBudget {
            max_iterations: self.rounds,
            max_tool_calls: self.calls,
        }
    }

    /// Reserve at most one quarter of a known window, leaving usable input on small models.
    pub fn reserved(&self, limit: Option<u64>) -> u32 {
        limit.map_or(self.output, |limit| {
            self.output
                .min(u32::try_from((limit / 4).max(1)).unwrap_or(u32::MAX))
        })
    }
}

#[derive(Clone, Debug)]
pub struct RunContext {
    pub entries: Vec<Entry>,
    pub summary: Option<Summary>,
    pub policy: Policy,
    pub reason: Option<String>,
    pub incomplete: bool,
    pub artifacts: Option<(PathBuf, Uuid, Uuid)>,
    /// False only when the engine declared the model cannot read images.
    pub vision: bool,
    /// The tokens a coding agent reported its last request on this chat
    /// filled, its own prompt and tools included, which zone's estimate of
    /// what it sends leaves out.
    pub reported: Option<u64>,
}

impl RunContext {
    pub fn from_messages(messages: Vec<Message>) -> Self {
        let latest = messages
            .iter()
            .rposition(|message| message.role == Role::User);
        Self {
            entries: messages
                .into_iter()
                .enumerate()
                .map(|(index, message)| {
                    let preserve = message.role == Role::System || Some(index) == latest;
                    Entry::new(Uuid::new_v4().to_string(), message)
                        .with_preserve(preserve)
                        .with_consumed(true)
                })
                .collect(),
            summary: None,
            policy: Policy::new(None, 4096, ContextSource::Unknown),
            reason: None,
            incomplete: false,
            artifacts: None,
            vision: true,
            reported: None,
        }
    }

    pub fn usage(
        &self,
        model: &str,
        tools: Option<&[abnegate_llm::ToolDefinition]>,
    ) -> ContextUsage {
        let mut usage = context::estimate(
            model,
            &self.entries,
            tools,
            &self.policy,
            self.summary.as_ref(),
        );
        self.decorate(&mut usage);
        usage
    }

    pub fn decorate(&self, usage: &mut ContextUsage) {
        if let Some(reported) = self.reported.filter(|reported| *reported > usage.used) {
            usage.used = reported;
            usage.remaining = usage
                .threshold
                .map(|threshold| threshold.saturating_sub(reported));
        }
        usage.incomplete |= self.incomplete;
        if let Some(reason) = &self.reason {
            usage.reason = Some(match usage.reason.take() {
                Some(existing) => format!("{existing} {reason}"),
                None => reason.clone(),
            });
        }
    }

    pub fn append(&mut self, entry: &NewEntry) {
        self.entries.push(Entry::new(
            entry.id.clone(),
            entry.message.clone().into_message(),
        ));
    }

    /// Request-local search data stays below trusted instructions and outside durable
    /// coverage. Replacing it cannot change which canonical user entry is protected.
    pub fn search(&mut self, search: &SearchContext) {
        let supplement = Entry::new(SEARCH, Message::user(search.prompt()))
            .with_preserve(true)
            .with_consumed(true);
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == SEARCH) {
            *entry = supplement;
        } else {
            self.entries.push(supplement);
        }
    }

    pub fn consume(&mut self) -> Vec<String> {
        self.entries
            .iter_mut()
            .filter(|entry| !entry.consumed)
            .map(|entry| {
                entry.consumed = true;
                entry.id.clone()
            })
            .collect()
    }

    /// Resolve protected images on a transport copy, never in canonical replay/hash fields.
    pub async fn transport(&self, messages: &mut [Message]) -> Result<(), String> {
        if !self.vision {
            messages
                .iter_mut()
                .filter(|message| !message.images.is_empty())
                .for_each(withhold_images);
        }
        for message in messages {
            for image in &mut message.images {
                if !image.starts_with("/api/artifacts/") {
                    continue;
                }
                let (root, workspace, chat) = self
                    .artifacts
                    .as_ref()
                    .ok_or("Protected image is unavailable outside its chat")?;
                let parts = image
                    .trim_start_matches("/api/artifacts/")
                    .split('/')
                    .collect::<Vec<_>>();
                if parts.len() != 4
                    || parts[0] != workspace.to_string()
                    || parts[1] != chat.to_string()
                {
                    return Err("Historical image does not belong to this chat".into());
                }
                let owner = Uuid::parse_str(parts[2])
                    .map_err(|_| "Historical image has an invalid owner")?;
                let mime = match parts[3].rsplit('.').next() {
                    Some("png") => "image/png",
                    Some("jpg" | "jpeg") => "image/jpeg",
                    Some("webp") => "image/webp",
                    _ => return Err("Historical image format is unsupported".into()),
                };
                let bytes = ArtifactStore::new(root.clone())
                    .read(*workspace, *chat, owner, parts[3])
                    .await
                    .map_err(|error| format!("Historical image could not be loaded: {error}"))?;
                *image = format!(
                    "data:{mime};base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                );
            }
        }
        Ok(())
    }
}

fn withhold_images(message: &mut Message) {
    message.images.clear();
    message.content = Some(match message.content.take() {
        Some(content) if !content.is_empty() => format!("{content}\n\n{WITHHELD_IMAGE}"),
        _ => WITHHELD_IMAGE.into(),
    });
}

/// An `auto` name is resolved later by whoever serves it, so only a concrete
/// model the engine declared text-only loses its images.
pub fn sees_images(model: &str, vision: Option<bool>) -> bool {
    crate::services::stages::is_auto(model) || vision != Some(false)
}

#[derive(Clone)]
pub enum Mode {
    Preview,
    /// A turn a model answers, on the backend its model was chosen for.
    Generation(Generation),
}

pub struct Preparation {
    pub model: String,
    pub agentic: bool,
    pub auto_approve: bool,
    /// `chats.agent_sandboxed`: whether a spawned coding agent is confined to
    /// zone's tools, or also keeps its own file and shell tools, which zone
    /// never sees and cannot gate. An axis of its own, independent of
    /// [`Self::auto_approve`].
    pub sandboxed: bool,
    pub tools: ChatTools,
    pub context: RunContext,
    pub llm: LlmClient,
    pub endpoint: Endpoint,
    pub stop: Vec<String>,
    pub budget: LoopBudget,
    pub timeout: Duration,
    /// Built once here so a preview and the generation that follows it, and
    /// the retrieval-augmented reassembly in `ws::chat`, all read one clock.
    pub environment: Environment,
    /// What the workspace has learned, bounded for a chat on this model's
    /// window and read once here so the reassembly in `ws::chat` composes the
    /// same bytes and retrieves none of the entries it already shows. Empty
    /// for a character card with its agent off.
    pub findings: learned::Block,
    /// What this user's memory adds to the system prompt, read once here so
    /// the reassembly in `ws::chat` composes the same bytes rather than a
    /// second read of a table that may have moved between the two.
    pub memory: String,
    /// The workspace's skills index, read once here for the same reason. Empty
    /// for a chat with no `read_document`, which could open none of them.
    pub skills: String,
    /// The coding agent session the turn runs in, on a login of the organization's.
    pub session: Option<Pinned>,
    /// The logins the turn may move to when the one it runs on can no longer run it: none over
    /// HTTP, or under the instance's or the host's sign-in.
    pub handover: Option<Handover>,
    /// Why the chat's project could not give this turn its directory, which the answer opens by
    /// saying.
    pub displaced: Option<String>,
    /// The chat's reasoning effort, which a coding agent the turn hands over to
    /// is told again.
    pub effort: ReasoningEffort,
    /// The coding agent the turn runs on and the model the chat asked it for,
    /// whose report of its window the chat keeps.
    pub driver: Option<Driver>,
}

/// Read-only common builder. It never classifies intent, executes tools, searches, or summarizes.
/// A pending draft is added only to this projection and is never persisted.
pub async fn build(
    state: &AppState,
    chat: &ChatRow,
    user: Uuid,
    pending: Option<(&str, Option<&Value>)>,
    mode: Mode,
    endpoint: Endpoint,
    driver: Option<Driver>,
) -> Result<Preparation, String> {
    let workspace = chat
        .workspace_id
        .ok_or("Chat has no workspace association")?;
    let settings = &state.config().chat;
    let store = Store::new(state.db().clone(), chat.id, Some(workspace));
    let scope = WorkspaceScope {
        state: state.clone(),
        workspace_id: workspace,
        chat_id: Some(chat.id),
        user_id: user,
        offline: chat.offline,
        origin: match &mode {
            Mode::Generation(generation) => generation.origin,
            Mode::Preview => Origin::Member,
        },
    };
    let directory = match &mode {
        Mode::Generation(generation) => generation.directory.clone(),
        Mode::Preview => match project_directory::peek(state, chat.id, workspace).await {
            Start::Directory(directory) => Some(directory),
            Start::Elsewhere | Start::Missing(_) | Start::Unavailable(_) => None,
        },
    };
    let catalog = async {
        let tools = if matches!(mode, Mode::Generation(_)) && chat.agent_enabled {
            ChatTools::build(scope).await
        } else {
            ChatTools::preview(scope).await
        };
        tools.starting_in(directory).await
    };
    let capacity = async {
        if let Some(driver) = &driver {
            return driver.capacity(state.db(), chat.id).await;
        }
        let requested = chat
            .context_tokens
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0);
        let capacity = endpoint
            .capacity(state.config())
            .resolve(&chat.model_name)
            .await
            .with_request(requested);
        (capacity, None)
    };
    let (history, (capacity, reported), tools) = tokio::join!(store.load(), capacity, catalog);
    if matches!(mode, Mode::Generation(_)) {
        endpoint
            .model(&chat.model_name)
            .map_err(|error| error.to_string())?;
    }
    let history = history.map_err(|error| error.to_string())?;
    // A planner chat carries the two calls that end its interview until it
    // has made its project; after that it is a chat about the project.
    let tools = match crate::db::chats::link(state.db(), chat.id).await {
        Ok(Some(link))
            if link.purpose == crate::db::chats::ChatPurpose::ProjectPlanner
                && link.project_id.is_none() =>
        {
            tools.with_planner()
        }
        _ => tools,
    };
    let backend = match &mode {
        Mode::Preview => backend::instance(state.config()),
        Mode::Generation(generation) => backend::effort(
            backend::ahead_of(generation.resolved.backend.clone(), settings.timeout),
            chat.reasoning_effort,
        ),
    };
    let session = match &mode {
        Mode::Preview => None,
        Mode::Generation(generation) => Pinned::of(generation),
    };
    let tools = crate::mcp::offered(&backend, tools);
    let agentic = chat.agent_enabled && !tools.is_empty();
    let policy = policy(settings, &capacity);
    let request = pending
        .map(|(content, _)| content.to_string())
        .or_else(|| latest_user(&history))
        .unwrap_or_default();
    let effort = capacity
        .reasoning
        .then(|| chat.reasoning_effort.resolve(&request))
        .flatten();
    let mut environment =
        Environment::here().with_directory(tools.working_directory().to_path_buf());
    match crate::db::users::time_zone(state.db(), user).await {
        Ok(Some(zone)) => environment = environment.with_time_zone(zone),
        Ok(None) => {}
        Err(error) => tracing::warn!(
            chat_id = %chat.id,
            %error,
            "Failed to read the person's time zone; stating the server's"
        ),
    }
    if let Some(effort) = effort {
        environment = environment.with_effort(effort);
    }
    if agentic {
        match crate::db::sources::has_active(state.db(), workspace, crate::db::sources::GITHUB)
            .await
        {
            Ok(true) => environment = environment.with_github(),
            Ok(false) => {}
            Err(error) => tracing::warn!(
                chat_id = %chat.id,
                %error,
                "Failed to read whether the workspace has a GitHub source; not naming its tools"
            ),
        }
    }
    let findings = if reads_learned(chat, agentic) {
        learned::prompt(state.db(), workspace, learned::budget(policy.input_limit())).await
    } else {
        learned::Block::default()
    };
    // Every chat reads what is remembered about the person it is talking to:
    // a profile and a set of preferences describe them and how they want to be
    // worked with, and a chat with its agent off is still a chat with them.
    // Only a chat holding the memory tools is given the fact index, which
    // names `memory_read` and is nothing without it.
    let recall = if agentic {
        crate::agent::memory::render::Recall::Indexed
    } else {
        crate::agent::memory::render::Recall::Passive
    };
    let memory =
        match crate::agent::memory::render::prompt(state.db(), recall, workspace, user).await {
            Ok(memory) => memory,
            Err(error) => {
                tracing::warn!(
                    chat_id = %chat.id,
                    %error,
                    "Failed to load remembered entries; continuing without them"
                );
                String::new()
            }
        };
    // A skill is opened with read_document, so only a chat that can call it
    // is shown the index: a plain chat, or one with its agent off, has no
    // loader in front of the model and is told of nothing.
    let skills = if agentic && tools.has("read_document") {
        match crate::agent::skills::prompt(state.db(), workspace).await {
            Ok(skills) => skills,
            Err(error) => {
                tracing::warn!(
                    chat_id = %chat.id,
                    %error,
                    "Failed to load the skills index; continuing without it"
                );
                String::new()
            }
        }
    } else {
        String::new()
    };
    let mut entries = vec![
        Entry::new(
            INSTRUCTIONS,
            Message::system(system_prompt(
                chat,
                &tools,
                agentic,
                &SearchContext::new(&state.config().web_search).capability(),
                &environment,
                &findings.text,
                &memory,
                &skills,
            )),
        )
        .with_preserve(true)
        .with_consumed(true),
    ];
    entries.extend(history.entries.into_iter().map(|entry| {
        let preserve = pending.is_none() && history.latest_user.as_ref() == Some(&entry.id)
            || entry.message.role == Role::System;
        Entry::new(entry.id, entry.message.into_message())
            .with_preserve(preserve)
            .with_consumed(entry.consumed)
    }));
    let mut incomplete = history.incomplete;
    let mut reason = capacity.reason;
    if let Some((content, metadata)) = pending {
        let mut message = Message::user(content);
        message.images = images(metadata);
        entries.push(Entry::new("draft", message).with_preserve(true));
        if state.config().web_search.requested_for(content, metadata) {
            incomplete = true;
            reason =
                Some("Requested search results will be counted when retrieval completes.".into());
        }
    }
    if matches!(mode, Mode::Preview)
        && agentic
        && state.existing_mcp().is_none()
        && !zone_core::servers::config().is_empty()
    {
        incomplete = true;
        reason = Some(
            "Configured MCP tool definitions will be counted when their servers connect.".into(),
        );
    }
    if matches!(mode, Mode::Preview) && !agentic && chat.character.is_none() {
        let knowledge: bool = sqlx::query_scalar(concat!("SELECT EXISTS(SELECT 1 FROM knowledge_entries WHERE workspace_id=$1 AND is_active=TRUE ", not_memory!(), ")"))
            .bind(workspace).fetch_one(state.db()).await.map_err(|error|error.to_string())?;
        let sources = if state.context_service().is_some() {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM content_items WHERE workspace_id=$1)",
            )
            .bind(workspace)
            .fetch_one(state.db())
            .await
            .map_err(|error| error.to_string())?
        } else {
            false
        };
        if knowledge || sources {
            incomplete = true;
            reason = Some(
                "Workspace retrieval results will be counted when retrieval completes.".into(),
            );
        }
    }
    if history.incomplete {
        reason = Some("Some legacy tool evidence was not retained in full.".into());
    }
    let stop = merge_stops(
        chat.character
            .as_ref()
            .map(|card| card.stop_sequences.as_slice())
            .unwrap_or(&[]),
    );
    let mut llm = endpoint
        .llm(
            chat.model_name.clone(),
            TEMPERATURE,
            policy.reserved,
            backend,
        )
        .with_stop(stop.clone());
    if let Some(limit) = capacity.ollama {
        llm = llm.with_ollama_context(&chat.model_name, limit);
    }
    if let Some(effort) = effort {
        llm = llm.with_reasoning(&chat.model_name, effort);
    }
    let mut context = RunContext {
        entries,
        summary: history.summary.map(core_summary),
        policy,
        reason,
        incomplete,
        artifacts: Some((
            state.config().comfyui.artifact_root.clone(),
            workspace,
            chat.id,
        )),
        vision: sees_images(&chat.model_name, capacity.vision),
        reported,
    };
    context.search(&SearchContext::new(&state.config().web_search));
    Ok(Preparation {
        model: chat.model_name.clone(),
        agentic,
        auto_approve: chat.auto_approve,
        sandboxed: chat.agent_sandboxed,
        tools,
        context,
        llm,
        endpoint,
        stop,
        budget: settings.budget(),
        timeout: settings.timeout,
        environment,
        findings,
        memory,
        skills,
        session,
        handover: None,
        displaced: None,
        effort: chat.reasoning_effort,
        driver,
    })
}

/// The content of the newest canonical user turn, read before `history.entries`
/// is moved into the projection.
fn latest_user(history: &History) -> Option<String> {
    let id = history.latest_user.as_ref()?;
    history
        .entries
        .iter()
        .find(|entry| &entry.id == id)
        .and_then(|entry| entry.message.content.clone())
}

pub fn policy(settings: &Settings, capacity: &Capacity) -> Policy {
    Policy::new(
        capacity.limit,
        settings.reserved(capacity.limit),
        capacity.source,
    )
}

/// Whether a chat is shown what its workspace has learned. A character card
/// with its agent off is the card and the boundary alone: it is playing a
/// part, not working in the repository those entries describe.
fn reads_learned(chat: &ChatRow, agentic: bool) -> bool {
    agentic || chat.character.is_none()
}

/// The whole system entry: a character card if there is one, the built prompt
/// for this surface, the web-search capability tail, then what the workspace
/// has learned, then what is remembered about this user, then the workspace's
/// skills index. The same order a run reads them in.
///
/// `findings`, `memory` and `skills` are required rather than optional so the
/// compiler enumerates every caller: a turn that quietly lost a block would
/// ask again what the workspace has already answered, answer a person the
/// server knows about as if it had never met them, or start work the
/// workspace has a written procedure for as if it had none.
///
/// A character card with its agent off reads no learned block whatever it is
/// handed, so no caller can give it one.
pub fn system_prompt(
    chat: &ChatRow,
    tools: &ChatTools,
    agentic: bool,
    capability: &str,
    environment: &Environment,
    findings: &str,
    memory: &str,
    skills: &str,
) -> String {
    let prompt = match (chat.character.as_ref(), agentic) {
        (Some(card), true) => format!(
            "{}\n\n{}",
            card.system_prompt(),
            crate::agent::prompt::chat(tools, chat.auto_approve, environment)
        ),
        (Some(card), false) => match crate::agent::prompt::boundary() {
            boundary if boundary.is_empty() => card.system_prompt(),
            boundary => format!("{}\n\n{boundary}", card.system_prompt()),
        },
        (None, true) => crate::agent::prompt::chat(tools, chat.auto_approve, environment),
        (None, false) => crate::agent::prompt::plain(environment),
    };
    let findings = if reads_learned(chat, agentic) {
        findings
    } else {
        ""
    };
    format!("{prompt}\n\n{capability}{findings}{memory}{skills}")
}

/// A minimal chat row, so prompt tests do not need a database.
///
/// The three shapes that matter are a card with the agent on, a card alone, and
/// a plain assistant chat.
#[cfg(test)]
pub(crate) fn chat_row(
    character: Option<crate::services::character::ChatCharacter>,
    agent_enabled: bool,
    auto_approve: bool,
) -> ChatRow {
    ChatRow {
        id: Uuid::new_v4(),
        workspace_id: Some(Uuid::new_v4()),
        title: "Prompt fixture".into(),
        model_name: "fixture-model".into(),
        archived: None,
        agent_enabled,
        agent_sandboxed: false,
        auto_approve,
        offline: false,
        context_tokens: None,
        reasoning_effort: ReasoningEffort::default(),
        character,
        created_at: None,
        updated_at: None,
    }
}

/// The system prompt of an agentic chat composed at `at`, carrying `memory`, so prompt tests
/// compare prompts a clock apart.
#[cfg(test)]
pub(crate) fn composed(at: &str, memory: &str) -> String {
    informed(at, "", memory)
}

/// The same prompt in a workspace that has learned `findings`.
#[cfg(test)]
pub(crate) fn informed(at: &str, findings: &str, memory: &str) -> String {
    system_prompt(
        &chat_row(None, true, false),
        &ChatTools::with_names(crate::agent::ToolProfile::Chat, &["read_file"], None),
        true,
        "Web search is unavailable this turn.",
        &Environment::at(
            chrono::DateTime::parse_from_rfc3339(at).expect("a timestamp"),
            "Pacific/Auckland",
            PathBuf::from("/srv/zone"),
        ),
        findings,
        memory,
        "",
    )
}

pub fn images(metadata: Option<&Value>) -> Vec<String> {
    metadata
        .and_then(|value| value.get("attachments"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|attachment| {
            attachment
                .get("mime")
                .and_then(Value::as_str)
                .is_some_and(|mime| mime.starts_with("image/"))
        })
        .filter_map(|attachment| attachment.get("url").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

pub fn core_summary(summary: chat::Summary) -> Summary {
    Summary::new(
        summary.content,
        Coverage::new(summary.entries, summary.fingerprint),
        summary.revision,
    )
}
pub fn stored_summary(summary: &Summary) -> chat::Summary {
    chat::Summary::new(
        summary.content.clone(),
        summary.coverage.entries.clone(),
        summary.coverage.fingerprint.clone(),
        summary.revision,
    )
}

pub struct Session {
    pub store: Store,
    pub lease: Lease,
    pub guard: Guard,
    pub turn: Uuid,
    closed: bool,
}

impl Session {
    pub async fn acquire(
        state: &AppState,
        chat: Uuid,
        workspace: Uuid,
        turn: Uuid,
    ) -> Result<Self, Error> {
        let store = Store::new(state.db().clone(), chat, Some(workspace));
        let lease = store.acquire(turn, LEASE_LIFETIME).await?;
        let guard = store.keep_alive(lease.clone(), LEASE_LIFETIME)?;
        Ok(Self {
            store,
            lease,
            guard,
            turn,
            closed: false,
        })
    }

    /// Close this turn without the lease it was opened under.
    ///
    /// The lease is gone, so the fenced routes refuse; this one moves only the
    /// turn this session opened, which belongs to this generation alone.
    pub async fn settle(
        &self,
        content: Option<&str>,
        metadata: Option<Value>,
        partial: Option<&ReplayMessage>,
    ) -> Result<bool, Error> {
        self.store
            .settle(self.turn, content, metadata, partial)
            .await
    }

    /// Release before acknowledging a terminal response so the next request can start.
    pub async fn close(&mut self) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        match self.store.recover(&self.lease).await {
            Ok(_) => {}
            Err(Error::LeaseLost) => {
                self.settle(None, None, None).await?;
            }
            Err(error) => return Err(error),
        }
        self.guard.stop().await;
        self.store.release(&self.lease).await?;
        self.closed = true;
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let store = self.store.clone();
        let lease = self.lease.clone();
        let turn = self.turn;
        // Cancellation during preparation must not hold an idle chat until expiry.
        tokio::spawn(async move {
            if matches!(store.recover(&lease).await, Err(Error::LeaseLost)) {
                let _ = store.settle(turn, None, None, None).await;
            }
            let _ = store.release(&lease).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zone_core::llm::LlmBackend;

    /// What `SearchContext::capability()` returns on its quieter arm. Non-empty
    /// on both arms, which is what keeps a memory block one blank line below it
    /// rather than two.
    const CAPABILITY: &str = "Web search is unavailable this turn.";

    const CARD: &str = "You are Ada, and you stay in character.";

    /// A stored profile, rendered the way a chat turn receives it.
    fn memory() -> String {
        crate::agent::memory::render::render(
            crate::agent::memory::render::Recall::Indexed,
            Some(&crate::agent::memory::render::memory_row(
                crate::db::memory::MemoryCategory::Profile,
                crate::db::memory::PROFILE_TITLE,
                "Ada, an electrical engineer in Wellington.",
            )),
            None,
            &[],
        )
    }

    const INSTRUCTION: &str = "Run cargo nextest from the runner directory.";

    const CONVENTION: &str = "Migrations are timestamped and named in snake_case.";

    const DISCOVERY: &str = "The disposable test database listens on 55471.";

    /// One entry of each learned family, rendered the way a chat turn receives
    /// them.
    fn learned_block() -> String {
        use crate::db::knowledge::{LearnedCategory, LearnedEntryRow};

        fn entry(title: &str, content: &str) -> LearnedEntryRow {
            LearnedEntryRow {
                id: Uuid::new_v4(),
                workspace_id: Uuid::nil(),
                title: title.to_string(),
                content: content.to_string(),
                tags: Vec::new(),
                updated_at: None,
            }
        }

        learned::render(
            &learned::Learned {
                instructions: vec![entry("How do I run the tests", INSTRUCTION)],
                facts: vec![learned::Facts {
                    category: LearnedCategory::RepositoryConvention,
                    entries: vec![entry("Migration names", CONVENTION)],
                }],
                discoveries: crate::db::knowledge::discovery::Listing {
                    rows: vec![entry("Test database port", DISCOVERY)],
                    recorded: 1,
                },
                unread: 0,
            },
            learned::MAX_RENDERED_BYTES,
        )
        .text
    }

    /// A fixed clock, so every prompt these tests compose is comparable with
    /// every other.
    fn environment() -> Environment {
        Environment::at(
            chrono::DateTime::parse_from_rfc3339("2026-09-09T09:30:00+12:00").unwrap(),
            "Pacific/Auckland",
            PathBuf::from("/srv/zone"),
        )
    }

    fn tools() -> ChatTools {
        ChatTools::with_names(
            crate::agent::ToolProfile::Chat,
            &["list_documents", "read_document", "read_file"],
            None,
        )
    }

    #[test]
    fn small_windows_keep_input_capacity_and_use_the_same_response_reserve() {
        let settings = Settings::default();
        assert_eq!(settings.reserved(Some(4096)), 1024);
        assert_eq!(settings.reserved(Some(32768)), 4096);
        assert_eq!(settings.reserved(None), 4096);
        let policy = Policy::new(
            Some(4096),
            settings.reserved(Some(4096)),
            ContextSource::Runtime,
        );
        assert_eq!(policy.limit.unwrap() - u64::from(policy.reserved), 3072);
    }

    #[test]
    fn configured_output_can_lower_but_never_exhaust_known_input() {
        let settings = Settings {
            output: 512,
            ..Settings::default()
        };
        assert_eq!(settings.reserved(Some(4096)), 512);
        assert_eq!(settings.budget(), LoopBudget::chat());
    }

    /// A workspace that has learned far more than any chat block shows.
    fn full_store() -> learned::Learned {
        use crate::db::knowledge::{LearnedCategory, LearnedEntryRow};

        fn entries(family: &str, count: usize) -> Vec<LearnedEntryRow> {
            (0..count)
                .map(|index| LearnedEntryRow {
                    id: Uuid::new_v4(),
                    workspace_id: Uuid::nil(),
                    title: format!("{family} {index:02}"),
                    content: format!("{family}-{index:02} {}", "x".repeat(300)),
                    tags: Vec::new(),
                    updated_at: None,
                })
                .collect()
        }

        let discoveries = entries("discovery", 20);
        learned::Learned {
            instructions: entries("instruction", 20),
            facts: LearnedCategory::ALL
                .into_iter()
                .map(|category| learned::Facts {
                    category,
                    entries: entries(category.as_str(), 20),
                })
                .collect(),
            discoveries: crate::db::knowledge::discovery::Listing {
                recorded: discoveries.len(),
                rows: discoveries,
            },
            unread: 0,
        }
    }

    /// A plain chat on a 4,096-token window whose system entry carries the
    /// learned block and nothing else of the workspace's, and whose turn's
    /// message is the only other entry, so nothing can be compacted to make
    /// room.
    #[tokio::test]
    async fn a_small_window_prepares_a_turn_with_a_full_learned_store() {
        let window = 4_096;
        let policy = Policy::new(
            Some(window),
            Settings::default().reserved(Some(window)),
            ContextSource::Runtime,
        );
        let input = policy.input_limit().expect("a known window");
        let turn = |findings: &str| {
            let system = system_prompt(
                &chat_row(None, false, false),
                &ChatTools::with_names(crate::agent::ToolProfile::Chat, &[], None),
                false,
                CAPABILITY,
                &environment(),
                findings,
                "",
                "",
            );
            [
                Entry::new(INSTRUCTIONS, Message::system(system))
                    .with_preserve(true)
                    .with_consumed(true),
                Entry::new(
                    "draft",
                    Message::user("Which branch do deploys go out from?"),
                )
                .with_preserve(true),
            ]
        };
        let ceiling = learned::render(&full_store(), learned::MAX_RENDERED_BYTES);
        assert!(
            context::estimate("fixture-model", &turn(&ceiling.text), None, &policy, None).used
                > input,
            "the block a large window carries has to overflow this one for the test to prove \
             anything"
        );

        let findings = learned::render(&full_store(), learned::budget(Some(input)));
        assert!(findings.text.len() <= learned::budget(Some(input)));
        let entries = turn(&findings.text);

        let prepared = context::prepare(
            &LlmClient::new(abnegate_llm::LlmConfig::default()).summarizer(),
            "fixture-model",
            &entries,
            None,
            &policy,
            None,
        )
        .await;

        let prepared = prepared.expect("the turn fits the window it runs in");
        assert!(prepared.usage.used <= input, "{:?}", prepared.usage);
    }

    const PIXEL: &str = "data:image/png;base64,iVBORw0KGgo=";

    fn illustrated() -> RunContext {
        let mut described = Message::user("What is in this picture?");
        described.images = vec![PIXEL.into()];
        let mut bare = Message::user("");
        bare.images = vec![PIXEL.into()];
        let mut screenshot = Message::tool_result("call-1", "Captured the page.");
        screenshot.images = vec![PIXEL.into()];
        RunContext::from_messages(vec![
            Message::system("Be helpful."),
            described,
            bare,
            screenshot,
            Message::user("Hello"),
        ])
    }

    fn transported(context: &RunContext) -> Vec<Message> {
        context
            .entries
            .iter()
            .map(|entry| entry.message.clone())
            .collect()
    }

    #[tokio::test]
    async fn transport_drops_images_for_a_model_without_vision() {
        let mut context = illustrated();
        context.vision = false;
        let mut messages = transported(&context);
        context.transport(&mut messages).await.unwrap();

        assert!(messages.iter().all(|message| message.images.is_empty()));
        assert_eq!(
            messages[1].content.as_deref(),
            Some(format!("What is in this picture?\n\n{WITHHELD_IMAGE}").as_str())
        );
        assert_eq!(messages[2].content.as_deref(), Some(WITHHELD_IMAGE));
        assert_eq!(
            messages[3].content.as_deref(),
            Some(format!("Captured the page.\n\n{WITHHELD_IMAGE}").as_str())
        );
        assert_eq!(messages[4].content.as_deref(), Some("Hello"));
        assert_eq!(messages[0].content.as_deref(), Some("Be helpful."));
        let body = serde_json::to_string(&messages).unwrap();
        assert!(!body.contains("image_url"), "{body}");
        assert!(
            context
                .entries
                .iter()
                .filter(|entry| !entry.message.images.is_empty())
                .count()
                == 3,
            "Canonical history must keep its images for a later vision model"
        );
    }

    #[tokio::test]
    async fn transport_keeps_images_for_a_vision_model() {
        let context = illustrated();
        assert!(context.vision);
        let mut messages = transported(&context);
        context.transport(&mut messages).await.unwrap();

        assert_eq!(
            messages
                .iter()
                .filter(|message| message.images == [PIXEL])
                .count(),
            3
        );
        let body = serde_json::to_string(&messages).unwrap();
        assert!(body.contains("image_url"), "{body}");
        assert!(!body.contains(WITHHELD_IMAGE), "{body}");
    }

    #[test]
    fn only_a_declared_text_only_model_loses_its_images() {
        assert!(!sees_images("qwen3:8b", Some(false)));
        assert!(sees_images("qwen3:8b", None));
        assert!(sees_images("gemma3:4b", Some(true)));
    }

    #[test]
    fn an_auto_model_keeps_its_images() {
        assert!(sees_images(crate::services::stages::AUTO, Some(false)));
        assert!(sees_images("", Some(false)));
        assert!(sees_images("Auto", None));
    }

    /// The wiring proof for the whole builder: whichever of the four arms runs,
    /// a character card prefixes the assembled text, the boundary section is
    /// present, and the web-search capability tail follows the assembled
    /// prompt. The tail is the last thing the model reads only while there is
    /// nothing remembered for this user, which is why the four calls here pass
    /// no memory and the block's own placement is pinned separately. A persona
    /// chat with the agent off gets the boundary alone rather than a whole
    /// prompt, which is the only arm the builder does not assemble, so it is
    /// the one most easily lost.
    #[test]
    fn every_arm_carries_the_boundary_and_the_capability_follows_the_prompt() {
        let environment = environment();
        let tools = tools();
        let card = crate::services::character::ChatCharacter {
            name: "Ada".into(),
            system_prompt: Some(CARD.into()),
            ..Default::default()
        };
        let boundary = crate::agent::prompt::boundary();
        assert!(!boundary.is_empty());

        let plain = system_prompt(
            &chat_row(None, false, false),
            &tools,
            false,
            CAPABILITY,
            &environment,
            "",
            "",
            "",
        );
        let agentic = system_prompt(
            &chat_row(None, true, false),
            &tools,
            true,
            CAPABILITY,
            &environment,
            "",
            "",
            "",
        );
        let persona = system_prompt(
            &chat_row(Some(card.clone()), false, false),
            &tools,
            false,
            CAPABILITY,
            &environment,
            "",
            "",
            "",
        );
        let persona_agentic = system_prompt(
            &chat_row(Some(card), true, false),
            &tools,
            true,
            CAPABILITY,
            &environment,
            "",
            "",
            "",
        );

        for prompt in [&plain, &agentic, &persona, &persona_agentic] {
            assert!(prompt.contains(&boundary), "{prompt}");
            assert!(prompt.ends_with(&format!("\n\n{CAPABILITY}")), "{prompt}");
        }

        assert!(plain.starts_with("You are Zone's assistant"), "{plain}");
        assert!(!plain.contains("You can call these tools"), "{plain}");

        assert!(agentic.starts_with("You are Zone's assistant"), "{agentic}");
        assert!(agentic.contains("You can call these tools:"), "{agentic}");
        assert!(agentic.contains("Workspace actions:"), "{agentic}");

        for prompt in [&persona, &persona_agentic] {
            assert!(prompt.starts_with(CARD), "{prompt}");
        }
        assert_eq!(persona, format!("{CARD}\n\n{boundary}\n\n{CAPABILITY}"));
        assert!(
            persona_agentic.contains("You can call these tools:"),
            "{persona_agentic}"
        );
        assert!(
            persona_agentic.find(CARD) < persona_agentic.find(&boundary),
            "{persona_agentic}"
        );
    }

    /// A chat with nothing learned and nothing remembered must read byte for
    /// byte as it did before either block existed, on every arm: both features
    /// are then invisible in a workspace that has learned nothing, to a user
    /// nothing is remembered about.
    #[test]
    fn an_empty_memory_block_leaves_every_arm_exactly_as_it_was() {
        let environment = environment();
        let tools = tools();
        let card = crate::services::character::ChatCharacter {
            name: "Ada".into(),
            system_prompt: Some(CARD.into()),
            ..Default::default()
        };
        let arms = [
            (
                chat_row(None, false, false),
                false,
                format!(
                    "{}\n\n{CAPABILITY}",
                    crate::agent::prompt::plain(&environment)
                ),
            ),
            (
                chat_row(None, true, false),
                true,
                format!(
                    "{}\n\n{CAPABILITY}",
                    crate::agent::prompt::chat(&tools, false, &environment)
                ),
            ),
            (
                chat_row(Some(card.clone()), false, false),
                false,
                format!(
                    "{CARD}\n\n{}\n\n{CAPABILITY}",
                    crate::agent::prompt::boundary()
                ),
            ),
            (
                chat_row(Some(card), true, false),
                true,
                format!(
                    "{CARD}\n\n{}\n\n{CAPABILITY}",
                    crate::agent::prompt::chat(&tools, false, &environment)
                ),
            ),
        ];

        for (chat, agentic, expected) in arms {
            assert_eq!(
                system_prompt(&chat, &tools, agentic, CAPABILITY, &environment, "", "", ""),
                expected
            );
        }
    }

    /// The block follows the capability with no separator of its own, because
    /// `render` brings the blank line that opens it.
    #[test]
    fn a_memory_block_follows_the_capability_tail() {
        let environment = environment();
        let tools = tools();
        let chat = chat_row(None, true, false);
        let memory = memory();
        assert!(!memory.is_empty());

        let without = system_prompt(&chat, &tools, true, CAPABILITY, &environment, "", "", "");
        let with = system_prompt(
            &chat,
            &tools,
            true,
            CAPABILITY,
            &environment,
            "",
            &memory,
            "",
        );

        assert_eq!(with, format!("{without}{memory}"));
        assert!(
            with.contains(&format!("{CAPABILITY}{memory}")),
            "the block carries its own blank line, so nothing separates it from the tail: {with}"
        );
        assert!(
            with.find(CAPABILITY) < with.find("Ada, an electrical engineer"),
            "{with}"
        );
        assert!(
            with.ends_with("Ada, an electrical engineer in Wellington."),
            "{with}"
        );
    }

    /// The index is the last thing in the entry: after memory, so what is
    /// remembered about the person is read before what the workspace wrote
    /// about how to work, and carrying its own blank line like memory does.
    #[test]
    fn the_skills_index_follows_the_memory_block() {
        let environment = environment();
        let tools = tools();
        let chat = chat_row(None, true, false);
        let memory = memory();
        let skills = crate::agent::skills::render(
            &[crate::db::knowledge::SkillRow {
                id: Uuid::new_v4(),
                title: "Deploy checklist".into(),
                head: "---\ndescription: Use when shipping to production.\n---\n".into(),
            }],
            1,
        );
        assert!(skills.starts_with("\n\n# Skills"));
        let without = system_prompt(
            &chat,
            &tools,
            true,
            CAPABILITY,
            &environment,
            "",
            &memory,
            "",
        );
        let with = system_prompt(
            &chat,
            &tools,
            true,
            CAPABILITY,
            &environment,
            "",
            &memory,
            &skills,
        );
        assert_eq!(with, format!("{without}{skills}"));
        assert!(
            with.find("Ada, an electrical engineer") < with.find("# Skills"),
            "{with}"
        );
        assert!(!with.contains("\n\n\n"), "{with}");
        assert!(with.ends_with("Use when shipping to production."), "{with}");
    }

    /// What the workspace has learned is the first block after the tail and
    /// the remembered block the next, the order a run reads them in: how the
    /// workspace wants its work done, then who the work is for.
    #[test]
    fn the_learned_block_sits_between_the_capability_tail_and_the_memory_block() {
        let environment = environment();
        let tools = tools();
        let chat = chat_row(None, true, false);
        let findings = learned_block();
        let memory = memory();
        assert!(
            findings.starts_with("\n\n# Standing instructions\n"),
            "{findings}"
        );

        let bare = system_prompt(&chat, &tools, true, CAPABILITY, &environment, "", "", "");
        let with = system_prompt(
            &chat,
            &tools,
            true,
            CAPABILITY,
            &environment,
            &findings,
            &memory,
            "",
        );

        assert_eq!(with, format!("{bare}{findings}{memory}"));
        assert!(
            with.contains(&format!("{CAPABILITY}{findings}{memory}")),
            "each block carries its own blank line, so nothing separates the three: {with}"
        );
        let positions = [
            CAPABILITY,
            "# Standing instructions",
            INSTRUCTION,
            "# Repository conventions",
            CONVENTION,
            "# Recorded while working here",
            DISCOVERY,
            "Ada, an electrical engineer",
        ]
        .map(|text| {
            with.find(text)
                .unwrap_or_else(|| panic!("{text:?} is missing from {with}"))
        });
        assert!(positions.is_sorted(), "{positions:?}: {with}");
        assert!(
            with.ends_with("Ada, an electrical engineer in Wellington."),
            "{with}"
        );
    }

    #[test]
    fn a_prompt_carrying_every_block_keeps_them_one_blank_line_apart() {
        let environment = environment();
        let tools = tools();
        let chat = chat_row(None, true, false);
        let findings = learned_block();
        let memory = memory();
        let skills = crate::agent::skills::render(
            &[crate::db::knowledge::SkillRow {
                id: Uuid::new_v4(),
                title: "Deploy checklist".into(),
                head: "---\ndescription: Use when shipping to production.\n---\n".into(),
            }],
            1,
        );

        let bare = system_prompt(&chat, &tools, true, CAPABILITY, &environment, "", "", "");
        let composed = system_prompt(
            &chat,
            &tools,
            true,
            CAPABILITY,
            &environment,
            &findings,
            &memory,
            &skills,
        );

        assert_eq!(composed, format!("{bare}{findings}{memory}{skills}"));
        for block in [&findings, &memory, &skills] {
            assert!(block.starts_with("\n\n"), "{block}");
            assert!(!block.ends_with('\n'), "{block}");
        }
        assert!(!composed.contains("\n\n\n"), "{composed}");
        assert!(
            composed.ends_with("Use when shipping to production."),
            "{composed}"
        );
    }

    /// The agent being off does not take the block from an assistant chat,
    /// which answers questions about the same workspace. It does take it from
    /// a character, which is the card and the boundary and nothing learned:
    /// whatever the caller hands over, that arm reads none of it.
    #[test]
    fn a_character_with_its_agent_off_reads_no_learned_block() {
        let environment = environment();
        let tools = tools();
        let card = crate::services::character::ChatCharacter {
            name: "Ada".into(),
            system_prompt: Some(CARD.into()),
            ..Default::default()
        };
        let findings = learned_block();
        let memory = memory();
        let boundary = crate::agent::prompt::boundary();

        assert!(!reads_learned(
            &chat_row(Some(card.clone()), false, false),
            false
        ));
        assert!(reads_learned(
            &chat_row(Some(card.clone()), true, false),
            true
        ));
        assert!(reads_learned(&chat_row(None, false, false), false));
        assert!(reads_learned(&chat_row(None, true, false), true));

        let persona = system_prompt(
            &chat_row(Some(card.clone()), false, false),
            &tools,
            false,
            CAPABILITY,
            &environment,
            &findings,
            &memory,
            "",
        );
        assert_eq!(
            persona,
            format!("{CARD}\n\n{boundary}\n\n{CAPABILITY}{memory}")
        );

        for (chat, agentic) in [
            (chat_row(None, false, false), false),
            (chat_row(None, true, false), true),
            (chat_row(Some(card), true, false), true),
        ] {
            let composed = system_prompt(
                &chat,
                &tools,
                agentic,
                CAPABILITY,
                &environment,
                &findings,
                &memory,
                "",
            );
            assert!(
                composed.ends_with(&format!("{CAPABILITY}{findings}{memory}")),
                "{composed}"
            );
        }
    }

    /// The capability is non-empty on both `SearchContext` arms, and it has to
    /// be: with an empty one the prompt would already close on a blank line and
    /// a block opening with its own would make three.
    #[test]
    fn a_prompt_carrying_memory_keeps_its_sections_one_blank_line_apart() {
        let environment = environment();
        let tools = tools();
        let memory = memory();
        let composed = system_prompt(
            &chat_row(None, true, false),
            &tools,
            true,
            CAPABILITY,
            &environment,
            "",
            &memory,
            "",
        );

        assert!(!CAPABILITY.is_empty());
        assert!(
            !composed.contains("\n\n\n"),
            "the capability must be non-empty for the block to sit one blank line below it: {composed}"
        );
        assert!(memory.starts_with("\n\n"), "{memory}");
        assert!(!composed.ends_with('\n'), "{composed}");
    }

    #[test]
    fn unrepresentable_timeout_is_rejected() {
        assert_eq!(parse_timeout(1).unwrap(), Duration::from_secs(1));
        assert_eq!(
            parse_timeout(31_536_000).unwrap(),
            Duration::from_secs(31_536_000)
        );
        if Instant::now()
            .checked_add(Duration::from_secs(u64::MAX))
            .is_none()
        {
            let error = parse_timeout(u64::MAX).expect_err("overflowing deadline");
            assert!(error.contains("ZONE_CHAT_TIMEOUT_SECONDS"), "{error}");
        }
    }

    #[tokio::test]
    async fn a_saved_endpoint_with_no_model_refuses_a_turn_but_still_previews() {
        use crate::db::{chats, organizations, workspaces};
        use crate::services::endpoint;
        use crate::services::stages::AUTO;
        use sqlx::PgPool;
        use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;

        const GATEWAY: &str = "http://127.0.0.1:9";
        let pool = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let suffix = Uuid::new_v4().simple().to_string();
        let organization = organizations::create_organization(&pool, "Unset", &suffix, None)
            .await
            .expect("an organization");
        let workspace =
            workspaces::create_workspace(&pool, organization.id, "Unset", &suffix, None)
                .await
                .expect("a workspace");
        sqlx::query(
            "INSERT INTO organization_ai_settings (organization_id, provider, litellm_host, completions_routed) VALUES ($1, $2, $3, true)",
        )
        .bind(organization.id)
        .bind(PROVIDER_SELF_HOSTED)
        .bind(GATEWAY)
        .execute(&pool)
        .await
        .expect("the organization's AI settings");
        let chat = chats::create_chat(&pool, Some(workspace.id), "Unset", AUTO, false, false)
            .await
            .expect("a chat on automatic model selection");
        let state = AppState::new(crate::state::test_config(), pool.clone(), None);
        let user = Uuid::new_v4();
        let endpoint = crate::services::route::Route::for_workspace(&state, workspace.id)
            .await
            .into_endpoint()
            .expect("a usable route");

        let turn = build(
            &state,
            &chat,
            user,
            None,
            Mode::Generation(Generation::unrouted(LlmBackend::Http)),
            endpoint.clone(),
            None,
        )
        .await;
        let preview = build(&state, &chat, user, None, Mode::Preview, endpoint, None).await;
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization.id)
            .execute(&pool)
            .await
            .expect("the organization to be removed");

        assert_eq!(
            turn.err(),
            Some(endpoint::Error::ModelUnset.to_string()),
            "a turn on a saved endpoint with no model must tell the reader to set one"
        );
        if let Err(error) = preview {
            panic!("the context meter must still preview a chat with no model: {error}");
        }
    }

    #[tokio::test]
    async fn a_chats_effort_reaches_an_agent_on_its_backend_and_an_endpoint_in_its_body() {
        use crate::db::{chats, organizations, workspaces};
        use abnegate_agent_cli::{AgentKind, CliSettings};
        use serde_json::json;
        use sqlx::PgPool;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        const MODEL: &str = "reasoner";
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/model/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{
                    "model_name": MODEL,
                    "litellm_params": {"model": "anthropic/claude-sonnet"},
                    "model_info": {"max_input_tokens": 200000, "supports_reasoning": true}
                }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "completion",
                "object": "chat.completion",
                "created": 0,
                "model": MODEL,
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "Thought hard."},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        let pool = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let suffix = Uuid::new_v4().simple().to_string();
        let organization = organizations::create_organization(&pool, "Effort", &suffix, None)
            .await
            .expect("an organization");
        let workspace =
            workspaces::create_workspace(&pool, organization.id, "Effort", &suffix, None)
                .await
                .expect("a workspace");
        let created = chats::create_chat(&pool, Some(workspace.id), "Effort", MODEL, false, false)
            .await
            .expect("a chat");
        let chat = chats::update_chat(
            &pool,
            created.id,
            None,
            None,
            None,
            None,
            Some(ReasoningEffort::Max),
            None,
            None,
        )
        .await
        .expect("the chat's effort to be saved")
        .expect("the chat");
        let mut config = crate::state::test_config();
        config.litellm_host = server.uri();
        config.ollama_host = server.uri();
        let state = AppState::new(config, pool.clone(), None);
        let user = Uuid::new_v4();
        let endpoint = crate::services::route::Route::for_workspace(&state, workspace.id)
            .await
            .into_endpoint()
            .expect("the instance's route");

        let agent = build(
            &state,
            &chat,
            user,
            None,
            Mode::Generation(Generation::unrouted(LlmBackend::cli(
                AgentKind::Claude,
                CliSettings::default(),
            ))),
            endpoint.clone(),
            None,
        )
        .await;
        let http = build(
            &state,
            &chat,
            user,
            None,
            Mode::Generation(Generation::unrouted(LlmBackend::Http)),
            endpoint,
            None,
        )
        .await;
        let answered = match &http {
            Ok(preparation) => Some(preparation.llm.chat(&[Message::user("Hi")], None).await),
            Err(_) => None,
        };
        let requests = server.received_requests().await.unwrap_or_default();
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization.id)
            .execute(&pool)
            .await
            .expect("the organization to be removed");

        let agent = agent.unwrap_or_else(|error| panic!("an agent turn prepares: {error}"));
        assert_eq!(agent.effort, ReasoningEffort::Max);
        let LlmBackend::Cli {
            agent: kind,
            settings,
            ..
        } = agent.llm.backend()
        else {
            panic!("an agent turn runs on its CLI");
        };
        assert_eq!(*kind, AgentKind::Claude);
        assert_eq!(
            settings.effort,
            Some(ReasoningEffort::Max),
            "the agent is told the chat's effort"
        );
        let LlmBackend::Cli {
            settings: budgeted, ..
        } = backend::ahead_of(
            LlmBackend::cli(AgentKind::Claude, CliSettings::default()),
            state.config().chat.timeout,
        )
        else {
            panic!("a CLI backend stays one");
        };
        assert_eq!(
            settings.timeout, budgeted.timeout,
            "the effort leaves the chat's budget in place"
        );

        let http = http.unwrap_or_else(|error| panic!("an endpoint turn prepares: {error}"));
        assert_eq!(http.effort, ReasoningEffort::Max);
        assert!(matches!(http.llm.backend(), LlmBackend::Http));
        let answered = answered.expect("the endpoint turn was sent");
        assert!(answered.is_ok(), "{answered:?}");
        let sent = requests
            .iter()
            .find(|request| request.url.path() == "/chat/completions")
            .expect("the endpoint was asked for a completion");
        let body: Value = serde_json::from_slice(&sent.body).expect("a JSON body");
        assert_eq!(
            body["reasoning_effort"],
            json!("high"),
            "an endpoint is sent the chat's effort at the highest level it takes: {body}"
        );
    }
}
