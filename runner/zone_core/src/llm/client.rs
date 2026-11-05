//! Completions from an OpenAI-compatible endpoint or a coding agent CLI on
//! this host, behind one client.

use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use abnegate_llm::finish_reason::STOP;
use abnegate_llm::{
    ChatResponse, ChatStreamChunk, Choice, Completion, CompletionProvider, CompletionRequest,
    Effort, Error, LlmConfig, Message, RequestOptions, StreamChoice, StreamDelta, ToolDefinition,
};
use futures::{Stream, StreamExt};

use super::provider::{
    AgentEvent, AgentKind, AgentStream, BuiltinTools, CliProvider, CliSettings, Occupancy, Toolset,
    activity,
};

/// The longest a buffered completion may take, from sending the request to
/// reading the last byte of its answer. A local model on a CPU can spend
/// many minutes on one long answer, so this matches the time an agent CLI's
/// turn is given rather than the crate's ten minutes.
pub const TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The longest a streamed completion may go without sending a byte, the wait
/// for the first included. A local model server loads the model before it
/// answers, which for a large one on a cold cache takes several minutes.
pub const READ_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// A completion in pieces, whichever backend produced it.
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatStreamChunk, Error>> + Send>>;

/// Where completions come from.
#[derive(Debug, Clone, Default)]
pub enum LlmBackend {
    /// The OpenAI-compatible endpoint at [`LlmConfig::base_url`].
    #[default]
    Http,
    /// A coding agent CLI on this host, run as a child process. A single-user
    /// self-host can point zone at the `claude` or `codex` it has already
    /// signed in and spend that subscription instead of a metered API key.
    Cli {
        agent: AgentKind,
        settings: Box<CliSettings>,
    },
}

impl LlmBackend {
    pub fn cli(agent: AgentKind, settings: CliSettings) -> Self {
        Self::Cli {
            agent,
            settings: Box::new(settings),
        }
    }
}

/// A chat client that answers from the endpoint its config names or, when
/// its backend says so, from a coding agent CLI on this host.
#[derive(Debug, Clone)]
pub struct LlmClient {
    endpoint: abnegate_llm::LlmClient,
    backend: LlmBackend,
    reported: Reported,
}

/// What a coding agent last reported of its context, and which agent did, shared by every clone
/// of the client that ran it.
#[derive(Debug, Clone, Default)]
struct Reported(Arc<Mutex<Option<(AgentKind, Occupancy)>>>);

impl Reported {
    fn record(&self, agent: AgentKind, occupancy: Occupancy) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some((agent, occupancy));
    }

    fn latest(&self) -> Option<(AgentKind, Occupancy)> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Refuse a turn that expects zone's tools to be callable.
///
/// A coding agent runs its own tool loop and has no way to reach zone's
/// registry, so tools handed to a CLI backend would simply not be offered to
/// the model. Silently answering without them looks like a model that chose
/// not to call anything, which is the one reading a caller must not be given.
fn refuse_tools(tools: Option<&[ToolDefinition]>, agent: AgentKind) -> Result<(), Error> {
    let Some(tools) = tools.filter(|tools| !tools.is_empty()) else {
        return Ok(());
    };

    let names = tools
        .iter()
        .map(|tool| tool.function.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    Err(Error::InvalidConfig(format!(
        "the {agent} CLI backend runs its own tools and cannot be offered zone's ({names})"
    )))
}

/// One agent event in the shape the agent loop already consumes.
///
/// Tool activity becomes reasoning rather than a tool-call delta. The agent
/// has already run those tools, so replaying them through zone's registry
/// would run each a second time -- but a turn that spends ten minutes reading
/// and editing files should not look to the reader like a turn doing nothing,
/// so what the agent reached for is listed as the thinking it was. Zone's own
/// tools reach the reader as cards when the agent calls them over MCP.
fn chunk(event: AgentEvent, provider: &str) -> Result<ChatStreamChunk, Error> {
    let mut delta = StreamDelta::default();
    let mut finished = false;
    let mut usage = None;
    let mut window = None;
    let mut session = None;

    match event {
        AgentEvent::Text(text) => delta.content = Some(text),
        AgentEvent::Tool(call) => {
            delta.reasoning_content = activity(&call);
        }
        AgentEvent::Usage(counts) => usage = Some(counts),
        AgentEvent::Window(reported) => window = Some(reported),
        AgentEvent::Session(announced) => session = Some(announced),
        AgentEvent::Occupancy(_) => {}
        AgentEvent::Finished {
            finish_reason: reported,
        } => {
            tracing::debug!(
                provider,
                reported = reported.as_deref().unwrap_or("none"),
                "agent finished its turn"
            );
            finished = true;
        }
        AgentEvent::Failed(message) => return Err(Error::agent(message)),
        AgentEvent::Limited(limit) => return Err(Error::limited(provider, limit)),
    }

    let choice = StreamChoice::new(0, delta);
    let choice = if finished {
        choice.with_finish_reason(STOP)
    } else {
        choice
    };
    let mut chunk = ChatStreamChunk::new(vec![choice]);
    chunk.usage = usage;
    chunk.window = window;
    chunk.session = session;
    Ok(chunk)
}

fn chunks(
    events: AgentStream,
    agent: AgentKind,
    reported: Reported,
) -> impl Stream<Item = Result<ChatStreamChunk, Error>> + Send {
    let provider = agent.to_string();
    async_stream::stream! {
        let mut events = events;
        while let Some(event) = events.next().await {
            let translated = match event {
                Ok(AgentEvent::Occupancy(occupancy)) => {
                    reported.record(agent, occupancy);
                    continue;
                }
                Ok(event) => chunk(event, &provider),
                Err(error) => Err(Error::from(error)),
            };
            match translated {
                Ok(chunk) => yield Ok(chunk),
                Err(error) => {
                    yield Err(error);
                    break;
                }
            }
        }
    }
}

/// A completion from an agent CLI, in the envelope an endpoint would have
/// returned. Nothing upstream issues an identifier or a timestamp for a local
/// child process, so both are this machine's.
fn response(completion: Completion, model: &str) -> ChatResponse {
    tracing::debug!(
        provider = completion.provider,
        reported = completion.finish_reason.as_deref().unwrap_or("none"),
        "agent finished its turn"
    );

    let answered = ChatResponse::new(vec![
        Choice::new(0, completion.message).with_finish_reason(STOP),
    ])
    .with_id(format!("{}-{}", completion.provider, uuid::Uuid::new_v4()))
    .with_created(chrono::Utc::now().timestamp())
    .with_model(model);
    match completion.usage {
        Some(usage) => answered.with_usage(usage),
        None => answered,
    }
}

impl LlmClient {
    /// A client for the endpoint `config` names, answering from it until
    /// [`Self::with_backend`] says otherwise.
    pub fn new(config: LlmConfig) -> Self {
        Self {
            endpoint: abnegate_llm::LlmClient::new(config),
            backend: LlmBackend::Http,
            reported: Reported::default(),
        }
    }

    /// The context window and fill the coding agent this client now runs on
    /// last reported, which no stream chunk has room to carry. A report from
    /// an agent the turn handed over from is not this one's.
    pub fn occupancy(&self) -> Option<Occupancy> {
        let LlmBackend::Cli { agent, .. } = &self.backend else {
            return None;
        };
        self.reported
            .latest()
            .filter(|(reporter, _)| reporter == agent)
            .map(|(_, occupancy)| occupancy)
    }

    /// Ask the provider to halt on these strings. Custom GGUFs often ignore
    /// their own end tokens unless the request repeats them.
    pub fn with_stop(mut self, stop: Vec<String>) -> Self {
        self.endpoint = self.endpoint.with_stop(stop);
        self
    }

    /// Serve a turn's own tools to a CLI backend's agent, and say whether it
    /// also keeps the tools it ships with.
    ///
    /// Attached after construction because a turn's toolset does not exist
    /// until the turn does: the token is minted against that turn's registry
    /// and its approval policy, and neither is built when the client is.
    ///
    /// An HTTP backend is left exactly as it was rather than refused. It
    /// spawns no child to configure, and its tools travel in the request
    /// itself, so there is nothing here for it to lose: a request it makes
    /// still carries whatever definitions it was given.
    pub fn with_toolset(mut self, toolset: Toolset, builtin_tools: BuiltinTools) -> Self {
        if let LlmBackend::Cli { settings, .. } = &mut self.backend {
            let current = std::mem::take(settings.as_mut());
            **settings = current
                .with_toolset(toolset)
                .with_builtin_tools(builtin_tools);
        }
        self
    }

    /// This client on `backend` instead, keeping everything else it was built
    /// with. A toolset attached to the old backend is not carried over: it was
    /// minted for that backend's agent, so a caller that still serves the turn
    /// tools attaches them again.
    pub fn with_backend(mut self, backend: LlmBackend) -> Self {
        self.backend = backend;
        self
    }

    /// Derive a client with a task-specific sampling temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.endpoint = self.endpoint.with_temperature(temperature);
        self
    }

    /// Set a verified Ollama route's context capacity, sent only with a
    /// request for `model`.
    pub fn with_ollama_context(mut self, model: impl Into<String>, limit: u64) -> Self {
        self.endpoint = self.endpoint.with_ollama_context(model, limit);
        self
    }

    /// Enable thinking at `effort` for requests to `model`.
    pub fn with_reasoning(mut self, model: impl Into<String>, effort: Effort) -> Self {
        self.endpoint = self.endpoint.with_reasoning(model, effort);
        self
    }

    /// Context compaction must stay a cheap structured rewrite.
    pub fn without_reasoning(mut self) -> Self {
        self.endpoint = self.endpoint.without_reasoning();
        self
    }

    /// Make a chat completion request against the configured model.
    pub async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatResponse, Error> {
        self.chat_with_model(&self.config().default_model, messages, tools)
            .await
    }

    /// Make a chat completion request against a specific model.
    pub async fn chat_with_model(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatResponse, Error> {
        self.chat_with_options(model, messages, tools, self.reserved())
            .await
    }

    /// Make a chat completion request against `model`, reserving
    /// `options.reserved` tokens for the answer.
    pub async fn chat_with_options(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        options: RequestOptions,
    ) -> Result<ChatResponse, Error> {
        let LlmBackend::Cli { agent, settings } = &self.backend else {
            return self
                .endpoint
                .chat_with_options(model, messages, tools, options)
                .await;
        };

        refuse_tools(tools, *agent)?;
        let completion = CliProvider::agent(*agent, settings.as_ref().clone())
            .complete(CompletionRequest::new(model, messages, options))
            .await?;
        Ok(response(completion, model))
    }

    /// Stream a chat completion from the configured model.
    pub async fn chat_stream(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatStream, Error> {
        self.chat_stream_with_model(&self.config().default_model, messages, tools)
            .await
    }

    /// Stream a chat completion from a specific model.
    pub async fn chat_stream_with_model(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatStream, Error> {
        self.chat_stream_with_options(model, messages, tools, self.reserved())
            .await
    }

    /// Stream a chat completion from `model`, reserving `options.reserved`
    /// tokens for the answer.
    pub async fn chat_stream_with_options(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        options: RequestOptions,
    ) -> Result<ChatStream, Error> {
        let LlmBackend::Cli { agent, settings } = &self.backend else {
            let stream = self
                .endpoint
                .chat_stream_with_options(model, messages, tools, options)
                .await?;
            return Ok(Box::pin(stream));
        };

        refuse_tools(tools, *agent)?;
        let events = CliProvider::agent(*agent, settings.as_ref().clone())
            .stream(CompletionRequest::new(model, messages, options))?;
        Ok(Box::pin(chunks(events, *agent, self.reported.clone())))
    }

    /// The endpoint settings, with any temperature a `with_` method set.
    pub fn config(&self) -> &LlmConfig {
        self.endpoint.config()
    }

    /// Where this client's completions come from.
    pub fn backend(&self) -> &LlmBackend {
        &self.backend
    }

    fn reserved(&self) -> RequestOptions {
        RequestOptions::new(self.config().maximum_tokens)
    }
}

#[cfg(test)]
mod tests;
