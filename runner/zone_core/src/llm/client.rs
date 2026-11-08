//! Completions from an OpenAI-compatible endpoint or a coding agent CLI on
//! this host, behind one client.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use abnegate_agent_cli::{AgentKind, AgentStream, CliProvider, CliSettings, Occupancy};
use abnegate_llm::{
    ChatResponse, ChatStreamChunk, Choice, Completion, CompletionRequest, Effort, Error, LlmConfig,
    Message, RequestOptions, ToolDefinition,
};
use futures::Stream;

use super::backend::LlmBackend;
use super::cli::{BuiltinTools, Observer, SignIn, Toolset, confined};
use reported::Reported;
use turn::{Turn, ended, failed};

mod provider;
mod reported;
mod turn;

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

/// A chat client that answers from the endpoint its config names or, when
/// its backend says so, from a coding agent CLI on this host.
#[derive(Debug, Clone)]
pub struct LlmClient {
    endpoint: abnegate_llm::LlmClient,
    backend: LlmBackend,
    reported: Reported,
    toolset: Option<Arc<Toolset>>,
    builtin_tools: BuiltinTools,
    observer: Option<Arc<dyn Observer>>,
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

/// The model to name to `agent`: `model` when the agent knows it, or none,
/// leaving the choice to the agent rather than failing at its API on a name
/// such as zone's own `auto` or a local model's.
fn known(agent: AgentKind, model: &str) -> &str {
    if agent.knows(model) {
        model
    } else {
        tracing::debug!(
            %agent,
            model,
            "the agent does not know this model, so it chooses its own"
        );
        ""
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
        Choice::new(0, completion.message)
            .with_finish_reason(ended(completion.finish_reason.as_deref())),
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
            toolset: None,
            builtin_tools: BuiltinTools::default(),
            observer: None,
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
        if matches!(self.backend, LlmBackend::Cli { .. }) {
            self.toolset = Some(Arc::new(toolset));
            self.builtin_tools = builtin_tools;
        }
        self
    }

    /// Hand each of the agent's own finished tool calls, and each update to
    /// a subagent it started, to `observer`. Like a toolset, it belongs to
    /// one turn and is not carried to another backend.
    pub fn with_observer(mut self, observer: Arc<dyn Observer>) -> Self {
        if matches!(self.backend, LlmBackend::Cli { .. }) {
            self.observer = Some(observer);
        }
        self
    }

    /// This client on `backend` instead, keeping everything else it was built
    /// with. A toolset or observer attached for the old backend is not
    /// carried over: it was minted for that backend's agent, so a caller that
    /// still serves the turn tools attaches them again.
    pub fn with_backend(mut self, backend: LlmBackend) -> Self {
        self.backend = backend;
        self.toolset = None;
        self.builtin_tools = BuiltinTools::default();
        self.observer = None;
        self
    }

    /// Zone's tools the agent is served this turn, if any.
    pub fn toolset(&self) -> Option<&Toolset> {
        self.toolset.as_deref()
    }

    /// Whether the agent keeps the tools it ships with this turn.
    pub fn builtin_tools(&self) -> BuiltinTools {
        self.builtin_tools
    }

    /// Who is told of the agent's own tool calls and subagents this turn.
    pub fn observer(&self) -> Option<&Arc<dyn Observer>> {
        self.observer.as_ref()
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

    /// This client as context compaction asks it for a summary: at
    /// temperature 0, with no stop sequence to cut the structured answer
    /// short, and without reasoning.
    pub fn summarizer(&self) -> Self {
        self.clone()
            .with_temperature(0.0)
            .with_stop(Vec::new())
            .without_reasoning()
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
        let LlmBackend::Cli {
            agent,
            settings,
            sign_in,
        } = &self.backend
        else {
            return self
                .endpoint
                .chat_with_options(model, messages, tools, options)
                .await;
        };

        refuse_tools(tools, *agent)?;
        let (turn, events) = self
            .start(*agent, settings, *sign_in, model, messages, options)
            .await?;
        Ok(response(turn.completion(events).await?, model))
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
        let LlmBackend::Cli {
            agent,
            settings,
            sign_in,
        } = &self.backend
        else {
            let stream = self
                .endpoint
                .chat_stream_with_options(model, messages, tools, options)
                .await?;
            return Ok(Box::pin(stream));
        };

        refuse_tools(tools, *agent)?;
        let (turn, events) = self
            .start(*agent, settings, *sign_in, model, messages, options)
            .await?;
        Ok(Box::pin(turn.chunks(events)))
    }

    /// Start `agent` on `model` under `settings` and `sign_in`, returning
    /// the turn its events are read through and the events themselves.
    async fn start(
        &self,
        agent: AgentKind,
        settings: &CliSettings,
        sign_in: SignIn,
        model: &str,
        messages: &[Message],
        options: RequestOptions,
    ) -> Result<(Turn, AgentStream), Error> {
        let events = self
            .provider(agent, settings)
            .stream(CompletionRequest::new(
                known(agent, model),
                messages,
                options,
            ))
            .await
            .map_err(failed)?;
        let turn = Turn::new(
            agent,
            sign_in,
            settings.working_directory.as_deref(),
            self.observer.clone(),
            self.reported.clone(),
        );
        Ok((turn, events))
    }

    /// The provider that runs this turn's `agent`, under `settings` confined
    /// as every run Zone starts is, with the turn's toolset attached.
    fn provider(&self, agent: AgentKind, settings: &CliSettings) -> CliProvider {
        CliProvider::agent(
            agent,
            confined(
                agent,
                settings.clone(),
                self.toolset.as_deref(),
                self.builtin_tools,
            ),
        )
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
pub(crate) mod tests;
