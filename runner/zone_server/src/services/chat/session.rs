//! Shared preparation, projection, and durable generation boundaries for both chat modes.

use base64::Engine;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;
use zone_core::context::{self, ContextSource, ContextUsage, Coverage, Entry, Policy, Summary};
use zone_core::llm::{LlmClient, LlmConfig, Message, Role};

use crate::agent::{ChatTools, LoopBudget, WorkspaceScope};
use crate::db::chats::ChatRow;
use crate::db::context::{Error, Guard, Lease, Store};
use crate::services::artifacts::ArtifactStore;
use crate::services::chat::{capacity, history};
use crate::services::completion_tokens::merge_stops;
use crate::services::searxng::SearchContext;
use crate::state::AppState;

pub const LEASE_LIFETIME: Duration = Duration::from_secs(30);

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
            timeout: Duration::from_secs(value("ZONE_CHAT_TIMEOUT_SECONDS", 1800)?),
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
                .map(|(index, message)| Entry {
                    id: Uuid::new_v4().to_string(),
                    preserve: message.role == Role::System || Some(index) == latest,
                    message,
                    consumed: true,
                })
                .collect(),
            summary: None,
            policy: Policy {
                limit: None,
                reserved: 4096,
                source: ContextSource::Unknown,
            },
            reason: None,
            incomplete: false,
            artifacts: None,
        }
    }

    pub fn usage(
        &self,
        model: &str,
        tools: Option<&[zone_core::llm::ToolDefinition]>,
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
        usage.incomplete |= self.incomplete;
        if let Some(reason) = &self.reason {
            usage.reason = Some(match usage.reason.take() {
                Some(existing) => format!("{existing} {reason}"),
                None => reason.clone(),
            });
        }
    }

    pub fn append(&mut self, entry: &history::NewEntry) {
        self.entries.push(Entry {
            id: entry.id.clone(),
            message: entry.message.clone().into_message(),
            preserve: false,
            consumed: false,
        });
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Preview,
    Generation,
}

pub struct Preparation {
    pub model: String,
    pub agentic: bool,
    pub auto_approve: bool,
    pub tools: ChatTools,
    pub context: RunContext,
    pub llm: LlmClient,
    pub stop: Vec<String>,
    pub budget: LoopBudget,
    pub timeout: Duration,
}

/// Read-only common builder. It never classifies intent, executes tools, searches, or summarizes.
/// A pending draft is added only to this projection and is never persisted.
pub async fn build(
    state: &AppState,
    chat: &ChatRow,
    user: Uuid,
    pending: Option<(&str, Option<&Value>)>,
    mode: Mode,
) -> Result<Preparation, String> {
    let workspace = chat
        .workspace_id
        .ok_or("Chat has no workspace association")?;
    let settings = &state.config().chat;
    let store = Store::new(state.db().clone(), chat.id, Some(workspace));
    let resolver = capacity::Resolver::with_context(
        &state.config().litellm_host,
        &state.config().litellm_key,
        &state.config().ollama_host,
        Some(settings.context),
    );
    let scope = WorkspaceScope {
        state: state.clone(),
        workspace_id: workspace,
        chat_id: chat.id,
        user_id: user,
    };
    let catalog = async {
        if mode == Mode::Generation && chat.agent_enabled {
            ChatTools::build(scope).await
        } else {
            ChatTools::preview(scope).await
        }
    };
    let (history, capacity, tools) =
        tokio::join!(store.load(), resolver.resolve(&chat.model_name), catalog);
    let history = history.map_err(|error| error.to_string())?;
    let agentic = chat.agent_enabled && !tools.is_empty();
    let policy = policy(settings, &capacity);
    let mut entries = vec![Entry {
        id: "instructions".into(),
        message: Message::system(system_prompt(
            chat,
            &tools,
            agentic,
            &SearchContext::new(&state.config().web_search).capability(),
        )),
        preserve: true,
        consumed: true,
    }];
    entries.extend(history.entries.into_iter().map(|entry| Entry {
        preserve: pending.is_none() && history.latest_user.as_ref() == Some(&entry.id)
            || entry.message.role == Role::System,
        id: entry.id,
        message: entry.message.into_message(),
        consumed: entry.consumed,
    }));
    let mut incomplete = history.incomplete;
    let mut reason = capacity.reason;
    if let Some((content, metadata)) = pending {
        let mut message = Message::user(content);
        message.images = images(metadata);
        entries.push(Entry {
            id: "draft".into(),
            message,
            preserve: true,
            consumed: false,
        });
        if state.config().web_search.requested_for(content, metadata) {
            incomplete = true;
            reason =
                Some("Requested search results will be counted when retrieval completes.".into());
        }
    }
    if mode == Mode::Preview
        && agentic
        && state.existing_mcp().is_none()
        && !zone_core::mcp::McpConfig::from_env().servers.is_empty()
    {
        incomplete = true;
        reason = Some(
            "Configured MCP tool definitions will be counted when their servers connect.".into(),
        );
    }
    if mode == Mode::Preview && !agentic && chat.character.is_none() {
        let knowledge: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_entries WHERE workspace_id=$1 AND is_active=TRUE)")
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
    let mut llm = LlmClient::new(LlmConfig {
        base_url: state.config().litellm_host.clone(),
        api_key: state.config().litellm_key.clone(),
        default_model: chat.model_name.clone(),
        temperature: 0.7,
        max_tokens: policy.reserved,
    })
    .with_stop(stop.clone());
    if let Some(limit) = capacity.ollama {
        llm = llm.with_ollama_context(&chat.model_name, limit);
    }
    Ok(Preparation {
        model: chat.model_name.clone(),
        agentic,
        auto_approve: chat.auto_approve,
        tools,
        context: RunContext {
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
        },
        llm,
        stop,
        budget: settings.budget(),
        timeout: settings.timeout,
    })
}

pub fn policy(settings: &Settings, capacity: &capacity::Capacity) -> Policy {
    Policy {
        limit: capacity.limit,
        reserved: settings.reserved(capacity.limit),
        source: match capacity.source {
            capacity::Source::Runtime => ContextSource::Runtime,
            capacity::Source::Configured => ContextSource::Configured,
            capacity::Source::Provider => ContextSource::Provider,
            capacity::Source::Unknown => ContextSource::Unknown,
        },
    }
}

pub fn system_prompt(chat: &ChatRow, tools: &ChatTools, agentic: bool, capability: &str) -> String {
    let prompt = match (chat.character.as_ref(), agentic) {
        (Some(card), true) => format!(
            "{}\n\n{}",
            card.system_prompt(),
            crate::agent::system_prompt(tools, chat.auto_approve)
        ),
        (Some(card), false) => card.system_prompt(),
        (None, true) => crate::agent::system_prompt(tools, chat.auto_approve),
        (None, false) => {
            "You are Zone's assistant, answering inside one of the user's workspaces.".into()
        }
    };
    format!("{prompt}\n\n{capability}")
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

pub fn core_summary(summary: history::Summary) -> Summary {
    Summary {
        content: summary.content,
        coverage: Coverage {
            entries: summary.entries,
            fingerprint: summary.fingerprint,
        },
        revision: summary.revision,
    }
}
pub fn stored_summary(summary: &Summary) -> history::Summary {
    history::Summary {
        content: summary.content.clone(),
        entries: summary.coverage.entries.clone(),
        fingerprint: summary.coverage.fingerprint.clone(),
        revision: summary.revision,
    }
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
    /// Release before acknowledging a terminal response so the next request can start.
    pub async fn close(&mut self) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        self.store.recover(&self.lease).await?;
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
        // Cancellation during preparation must not hold an idle chat until expiry.
        tokio::spawn(async move {
            let _ = store.recover(&lease).await;
            let _ = store.release(&lease).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_windows_keep_input_capacity_and_use_the_same_response_reserve() {
        let settings = Settings::default();
        assert_eq!(settings.reserved(Some(4096)), 1024);
        assert_eq!(settings.reserved(Some(32768)), 4096);
        assert_eq!(settings.reserved(None), 4096);
        let policy = Policy {
            limit: Some(4096),
            reserved: settings.reserved(Some(4096)),
            source: ContextSource::Runtime,
        };
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
}
