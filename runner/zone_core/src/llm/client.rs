//! LLM client for OpenAI-compatible APIs

use futures::{Stream, StreamExt};
use reqwest::{Client, ClientBuilder, Url, redirect};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::runtime;

use crate::secret::{conceal, redact};

use super::dialect::{Budget, Dialect};
use super::finish_reason::STOP;
use super::metadata;
use super::provider::{
    AgentEvent, AgentKind, AgentStream, BuiltinTools, CliProvider, CliSettings, Completion,
    CompletionProvider, CompletionRequest, Toolset,
};
use super::types::{
    ChatRequest, ChatResponse, ChatStreamChunk, Choice, Message, StreamChoice, StreamDelta,
    ToolDefinition,
};

const REPORTED_LIMIT: usize = 500;
const ELLIPSIS: char = '…';

fn guarded(builder: ClientBuilder, trust: Trust) -> ClientBuilder {
    match trust {
        Trust::Operator => builder,
        Trust::Tenant => builder
            .no_proxy()
            .dns_resolver(Arc::new(metadata::Resolver))
            .redirect(redirect::Policy::none()),
    }
}

fn pool(trust: Trust) -> Client {
    let tuned = Client::builder()
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(10))
        .tcp_nodelay(true);
    guarded(tuned, trust)
        .build()
        .or_else(|_| guarded(Client::builder(), trust).build())
        .unwrap_or_else(|error| panic!("no HTTP client can be built: {error}"))
}

/// One connection pool per runtime. Building a `reqwest::Client` per turn
/// throws away TLS sessions and keep-alives to LiteLLM, which is the whole
/// time-to-first-token budget on a local model, so completions share one.
///
/// They cannot share more widely than the runtime. Every pooled connection is
/// driven by a task belonging to the runtime that opened it, so a pool reused
/// from a second runtime hands out connections whose driver died with the
/// first, and the send fails with "runtime dropped the dispatch task" without
/// ever reaching the server.
static HTTP: LazyLock<Mutex<HashMap<(runtime::Id, Trust), Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn http(trust: Trust) -> Client {
    let Ok(runtime) = runtime::Handle::try_current() else {
        return pool(trust);
    };
    let mut pools = HTTP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    pools
        .entry((runtime.id(), trust))
        .or_insert_with(|| pool(trust))
        .clone()
}

/// LLM client error
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {status} - {message}")]
    Api { status: u16, message: String },
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Stream error: {0}")]
    Stream(String),
    /// A coding agent CLI's own report of what went wrong, rendered in its own
    /// words so the operator reads "Not logged in" rather than a status code.
    #[error("{0}")]
    Agent(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

impl LlmError {
    /// Whether the provider explicitly rejected this model's tool capability.
    /// LiteLLM wraps Ollama's capability rejection as APIConnectionError (500).
    pub fn unsupported_tools(&self) -> bool {
        let Self::Api {
            status: 400 | 500,
            message,
        } = self
        else {
            return false;
        };
        let Ok(error) = serde_json::from_str::<serde_json::Value>(message) else {
            return false;
        };
        error
            .pointer("/error/message")
            .or_else(|| error.get("error"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|message| message.contains("does not support tools"))
    }
}

/// A completion in pieces, whichever backend produced it.
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatStreamChunk, LlmError>> + Send>>;

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
        settings: CliSettings,
    },
}

impl LlmBackend {
    pub fn cli(agent: AgentKind, settings: CliSettings) -> Self {
        Self::Cli { agent, settings }
    }
}

/// Who chose [`LlmConfig::base_url`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Trust {
    /// The operator, in the instance's own configuration.
    #[default]
    Operator,
    /// A tenant, in settings saved through the API. Its endpoint is never
    /// sent to a link-local or cloud metadata address, and what it answers a
    /// refused request with is reported only as its status and error message.
    Tenant,
}

/// Configuration for the LLM client
#[derive(Clone)]
pub struct LlmConfig {
    /// Base URL for the API (e.g., "https://api.openai.com/v1")
    pub base_url: String,
    /// API key for authentication
    pub api_key: String,
    /// Default model to use
    pub default_model: String,
    /// Default temperature
    pub temperature: f32,
    /// Default max tokens
    pub max_tokens: u32,
    /// Where completions are fetched from. Defaults to the endpoint above.
    pub backend: LlmBackend,
    pub dialect: Dialect,
    pub trust: Trust,
}

impl LlmConfig {
    pub fn with_backend(mut self, backend: LlmBackend) -> Self {
        self.backend = backend;
        self
    }
}

/// The key is the one field here that must never be printed, and this config
/// is embedded in the client that every worker logs.
impl std::fmt::Debug for LlmConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LlmConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &crate::secret::REDACTED)
            .field("default_model", &self.default_model)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field("backend", &self.backend)
            .field("dialect", &self.dialect)
            .field("trust", &self.trust)
            .finish()
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            default_model: "gpt-4".to_string(),
            temperature: 0.7,
            max_tokens: 4096,
            backend: LlmBackend::Http,
            dialect: Dialect::Compatible,
            trust: Trust::Operator,
        }
    }
}

/// Per-request output reservation. Must match the context budget calculation.
#[derive(Debug, Clone, Copy)]
pub struct RequestOptions {
    pub reserved: u32,
}

/// LLM client for making chat completion requests
#[derive(Debug, Clone)]
pub struct LlmClient {
    client: Client,
    config: LlmConfig,
    stop: Vec<String>,
    ollama: Option<(String, u64)>,
    /// Alias and resolved effort. Summarization clones the client and clears this.
    reasoning: Option<(String, crate::llm::Effort)>,
}

/// Check an outbound request URL and hand back the URL to request, so the
/// caller can only send the value that was checked.
///
/// No private-network rule here on purpose. Every caller passes an
/// operator-configured host, and a self-hosted Zone points at loopback,
/// a LAN address or a compose service name. The check belongs where a
/// tenant-supplied host is first accepted, against that value.
fn validate_outbound_url(url: &str) -> Result<Url, LlmError> {
    let parsed = Url::parse(url).map_err(|_| {
        LlmError::InvalidConfig("LLM base_url must be a valid absolute URL".to_string())
    })?;

    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(LlmError::InvalidConfig(
                "LLM base_url must use http or https".to_string(),
            ));
        }
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(LlmError::InvalidConfig(
            "LLM base_url must not include userinfo".to_string(),
        ));
    }

    if parsed.host_str().is_none() {
        return Err(LlmError::InvalidConfig(
            "LLM base_url must include a host".to_string(),
        ));
    }

    Ok(parsed)
}

/// The error message a refusal's body carries, as OpenAI, Anthropic, LiteLLM
/// and Ollama each shape it.
fn reported(body: &str) -> Option<String> {
    let body = serde_json::from_str::<serde_json::Value>(body).ok()?;
    ["/error/message", "/error", "/message"]
        .iter()
        .find_map(|pointer| body.pointer(pointer).and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_string)
}

fn capped(message: &str) -> String {
    match message.char_indices().nth(REPORTED_LIMIT) {
        Some((end, _)) => format!("{}{ELLIPSIS}", &message[..end]),
        None => message.to_string(),
    }
}

/// Refuse a turn that expects zone's tools to be callable.
///
/// A coding agent runs its own tool loop and has no way to reach zone's
/// registry, so tools handed to a CLI backend would simply not be offered to
/// the model. Silently answering without them looks like a model that chose
/// not to call anything, which is the one reading a caller must not be given.
fn refuse_tools(tools: Option<&[ToolDefinition]>, agent: AgentKind) -> Result<(), LlmError> {
    let Some(tools) = tools.filter(|tools| !tools.is_empty()) else {
        return Ok(());
    };

    let names = tools
        .iter()
        .map(|tool| tool.function.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    Err(LlmError::InvalidConfig(format!(
        "the {agent} CLI backend runs its own tools and cannot be offered zone's ({names})"
    )))
}

/// One agent event in the shape the agent loop already consumes.
///
/// Tool activity becomes reasoning rather than a tool-call delta. The agent
/// has already run those tools, so replaying them through zone's registry
/// would run each a second time -- but a turn that spends ten minutes reading
/// and editing files should not look to the reader like a turn doing nothing,
/// so what the agent reached for is shown as the thinking it was.
fn chunk(event: AgentEvent, provider: &str) -> Result<ChatStreamChunk, LlmError> {
    let mut delta = StreamDelta::default();
    let mut finish_reason = None;
    let mut usage = None;

    match event {
        AgentEvent::Text(text) => delta.content = Some(text),
        AgentEvent::Tool(call) => {
            delta.reasoning_content = Some(format!("{}\n", call.function.name));
        }
        AgentEvent::Usage(counts) => usage = Some(counts),
        AgentEvent::Finished {
            finish_reason: reported,
        } => {
            tracing::debug!(
                provider,
                reported = reported.as_deref().unwrap_or("none"),
                "agent finished its turn"
            );
            finish_reason = Some(STOP.to_string());
        }
        AgentEvent::Failed(message) => return Err(LlmError::Agent(message)),
    }

    Ok(ChatStreamChunk {
        id: None,
        object: None,
        created: None,
        model: None,
        choices: vec![StreamChoice {
            index: 0,
            delta,
            finish_reason,
        }],
        usage,
    })
}

fn chunks(
    events: AgentStream,
    provider: String,
) -> impl Stream<Item = Result<ChatStreamChunk, LlmError>> + Send {
    async_stream::stream! {
        let mut events = events;
        while let Some(event) = events.next().await {
            let translated = match event {
                Ok(event) => chunk(event, &provider),
                Err(error) => Err(LlmError::Agent(error.to_string())),
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

    ChatResponse {
        id: format!("{}-{}", completion.provider, uuid::Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model: model.to_string(),
        choices: vec![Choice {
            index: 0,
            message: completion.message,
            finish_reason: Some(STOP.to_string()),
        }],
        usage: completion.usage,
    }
}

impl LlmClient {
    /// Create a new LLM client
    pub fn new(config: LlmConfig) -> Self {
        Self {
            client: http(config.trust),
            config,
            stop: Vec::new(),
            ollama: None,
            reasoning: None,
        }
    }

    /// Ask the provider to halt on these strings. Custom GGUFs often ignore
    /// their own end tokens unless the request repeats them.
    pub fn with_stop(mut self, stop: Vec<String>) -> Self {
        self.stop = stop;
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
        if let LlmBackend::Cli { settings, .. } = &mut self.config.backend {
            *settings = std::mem::take(settings)
                .with_toolset(toolset)
                .with_builtin_tools(builtin_tools);
        }
        self
    }

    /// Derive a client with a task-specific sampling temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.config.temperature = temperature;
        self
    }

    /// Set a verified Ollama route's context capacity. The alias guard prevents
    /// forwarding provider-specific options after changing models. LiteLLM
    /// accepts num_ctx as a top-level non-OpenAI option (verified 1.99.1).
    pub fn with_ollama_context(mut self, model: impl Into<String>, limit: u64) -> Self {
        self.ollama = Some((model.into(), limit));
        self
    }

    /// Enable thinking for this alias. LiteLLM 1.99.1 maps `reasoning_effort`
    /// to Ollama `think`, Anthropic extended thinking, and OpenAI o-series.
    pub fn with_reasoning(mut self, model: impl Into<String>, effort: crate::llm::Effort) -> Self {
        self.reasoning = Some((model.into(), effort));
        self
    }

    /// Context compaction must stay a cheap structured rewrite.
    pub fn without_reasoning(mut self) -> Self {
        self.reasoning = None;
        self
    }

    fn body(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        reserved: u32,
        stream: bool,
    ) -> Result<serde_json::Value, LlmError> {
        let dialect = self.config.dialect;
        let stop = dialect.stops(model, &self.stop);
        let (max_tokens, max_completion_tokens) = match dialect.budget(model) {
            Budget::MaxTokens => (Some(reserved), None),
            Budget::MaxCompletionTokens => (None, Some(reserved)),
        };
        self.request(ChatRequest {
            model,
            messages,
            tools,
            tool_choice: None,
            temperature: dialect.temperature(model, self.config.temperature),
            max_tokens,
            max_completion_tokens,
            stream: Some(stream),
            stop: (!stop.is_empty()).then_some(stop.as_slice()),
        })
    }

    fn request(&self, request: ChatRequest<'_>) -> Result<serde_json::Value, LlmError> {
        let dialect = self.config.dialect;
        let mut body = serde_json::to_value(&request)?;
        if let Some((model, limit)) = &self.ollama
            && model == request.model
            && dialect.extended()
        {
            body["num_ctx"] = (*limit).into();
        }
        if let Some((model, effort)) = &self.reasoning
            && model == request.model
            && dialect.effort(model)
        {
            body["reasoning_effort"] = effort.as_str().into();
        }
        if request.stream == Some(true) {
            body["stream_options"] = serde_json::json!({ "include_usage": true });
        }
        Ok(body)
    }

    async fn send(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, LlmError> {
        let url = validate_outbound_url(url)?;
        if self.config.trust == Trust::Tenant && metadata::is_url(&url) {
            return Err(LlmError::InvalidConfig(
                "LLM base_url must not point at a link-local or cloud metadata address".to_string(),
            ));
        }
        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json");
        if !self.config.api_key.trim().is_empty() {
            request = request.bearer_auth(&self.config.api_key);
        }
        Ok(request.json(body).send().await?)
    }

    /// The endpoint's refusal, without the key this client sent it. A
    /// tenant's endpoint is reported only by its status and its own error
    /// message, in OpenAI's error envelope, so whatever else a host answers
    /// is never reflected back.
    async fn rejected(&self, response: reqwest::Response) -> LlmError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = match self.config.trust {
            Trust::Operator => conceal(&body, &self.config.api_key),
            Trust::Tenant => {
                let reported = reported(&body)
                    .unwrap_or_else(|| status.canonical_reason().unwrap_or_default().to_string());
                let message = capped(&redact(&conceal(&reported, &self.config.api_key)));
                serde_json::json!({ "error": { "message": message } }).to_string()
            }
        };
        LlmError::Api {
            status: status.as_u16(),
            message,
        }
    }

    /// Make a chat completion request
    pub async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatResponse, LlmError> {
        self.chat_with_model(&self.config.default_model, messages, tools)
            .await
    }

    /// Make a chat completion request with a specific model
    pub async fn chat_with_model(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatResponse, LlmError> {
        self.chat_with_options(
            model,
            messages,
            tools,
            RequestOptions {
                reserved: self.config.max_tokens,
            },
        )
        .await
    }

    pub async fn chat_with_options(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        options: RequestOptions,
    ) -> Result<ChatResponse, LlmError> {
        if let LlmBackend::Cli { agent, settings } = &self.config.backend {
            refuse_tools(tools, *agent)?;
            let completion = CliProvider::agent(*agent, settings.clone())
                .complete(CompletionRequest {
                    model,
                    messages,
                    tools: None,
                    options,
                })
                .await
                .map_err(|error| LlmError::Agent(error.to_string()))?;
            return Ok(response(completion, model));
        }

        let body = self.body(model, messages, tools, options.reserved, false)?;
        let url = format!("{}/chat/completions", self.config.base_url);

        let response = self.send(&url, &body).await?;

        if !response.status().is_success() {
            return Err(self.rejected(response).await);
        }

        let response: ChatResponse = response.json().await?;
        Ok(response)
    }

    /// Make a streaming chat completion request
    pub async fn chat_stream(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatStream, LlmError> {
        self.chat_stream_with_model(&self.config.default_model, messages, tools)
            .await
    }

    /// Make a streaming chat completion request with a specific model
    pub async fn chat_stream_with_model(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatStream, LlmError> {
        self.chat_stream_with_options(
            model,
            messages,
            tools,
            RequestOptions {
                reserved: self.config.max_tokens,
            },
        )
        .await
    }

    pub async fn chat_stream_with_options(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        options: RequestOptions,
    ) -> Result<ChatStream, LlmError> {
        if let LlmBackend::Cli { agent, settings } = &self.config.backend {
            refuse_tools(tools, *agent)?;
            let events = CliProvider::agent(*agent, settings.clone())
                .stream(CompletionRequest {
                    model,
                    messages,
                    tools: None,
                    options,
                })
                .map_err(|error| LlmError::Agent(error.to_string()))?;
            return Ok(Box::pin(chunks(events, agent.to_string())));
        }

        let body = self.body(model, messages, tools, options.reserved, true)?;
        let url = format!("{}/chat/completions", self.config.base_url);

        let response = self.send(&url, &body).await?;

        if !response.status().is_success() {
            return Err(self.rejected(response).await);
        }

        // Parse SSE without reallocating the leftover buffer on every line.
        let stream = async_stream::stream! {
            let mut bytes = response.bytes_stream();
            let mut buffer = Vec::<u8>::new();

            while let Some(chunk) = bytes.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        yield Err(LlmError::Http(e));
                        break;
                    }
                };

                buffer.extend_from_slice(&chunk);
                let mut consumed = 0usize;
                let mut done = false;
                while let Some(rel) = buffer[consumed..].iter().position(|&b| b == b'\n') {
                    let end = consumed + rel;
                    let mut line = &buffer[consumed..end];
                    if let Some(without_cr) = line.strip_suffix(b"\r") {
                        line = without_cr;
                    }
                    consumed = end + 1;
                    if line.is_empty() {
                        continue;
                    }
                    if let Some(data) = line.strip_prefix(b"data: ") {
                        if data == b"[DONE]" {
                            done = true;
                            break;
                        }
                        match serde_json::from_slice::<ChatStreamChunk>(data) {
                            Ok(chunk) => yield Ok(chunk),
                            Err(e) => yield Err(LlmError::Json(e)),
                        }
                    }
                }
                if consumed > 0 {
                    buffer.drain(..consumed);
                }
                if done {
                    break;
                }
            }
        };

        Ok(Box::pin(stream))
    }

    /// Get the current configuration
    pub fn config(&self) -> &LlmConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod authorization {
        use super::*;
        use serde_json::json;
        use wiremock::matchers::{header, method, path};
        use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

        struct Unauthorized;

        impl Match for Unauthorized {
            fn matches(&self, request: &Request) -> bool {
                !request.headers.contains_key(reqwest::header::AUTHORIZATION)
            }
        }

        fn completion() -> ResponseTemplate {
            ResponseTemplate::new(200).set_body_json(json!({
                "id": "completion",
                "object": "chat.completion",
                "created": 0,
                "model": "test",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "hi"},
                    "finish_reason": "stop"
                }]
            }))
        }

        async fn turn(server: &MockServer, api_key: &str) -> Result<ChatResponse, LlmError> {
            LlmClient::new(LlmConfig {
                base_url: server.uri(),
                api_key: api_key.to_string(),
                default_model: "test".to_string(),
                ..LlmConfig::default()
            })
            .chat(&[Message::user("hi")], None)
            .await
        }

        #[tokio::test]
        async fn an_endpoint_without_a_key_is_sent_no_authorization_header() {
            for blank in ["", "   "] {
                let server = MockServer::start().await;
                Mock::given(method("POST"))
                    .and(path("/chat/completions"))
                    .and(Unauthorized)
                    .respond_with(completion())
                    .expect(1)
                    .mount(&server)
                    .await;

                let answered = turn(&server, blank).await;

                assert!(answered.is_ok(), "{blank:?}: {answered:?}");
                server.verify().await;
            }
        }

        #[tokio::test]
        async fn an_endpoint_with_a_key_is_sent_it_as_a_bearer_token() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(header("authorization", "Bearer sk-endpoint-key"))
                .respond_with(completion())
                .expect(1)
                .mount(&server)
                .await;

            let answered = turn(&server, "sk-endpoint-key").await;

            assert!(answered.is_ok(), "{answered:?}");
            server.verify().await;
        }
    }

    mod refusal {
        use super::*;
        use wiremock::matchers::any;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        const KEY: &str = "sk-proj-AbCdEfGh1234567890wxyz";

        fn echoes() -> Vec<String> {
            vec![
                format!(r#"{{"error":{{"message":"bad key {KEY}"}}}}"#),
                r#"{"error":{"message":"Incorrect API key provided: sk-proj-****************wxyz."}}"#
                    .to_string(),
                r#"{"error":"{\"message\":\"invalid x-api-key: ****wxyz\"}"}"#.to_string(),
                r#"{"detail":"litellm.AuthenticationError: api_key=sk-proj-AbCd****"}"#.to_string(),
                "rejected key:sk-proj-AbCd****".to_string(),
            ]
        }

        async fn refused(body: &str, stream: bool) -> LlmError {
            let server = MockServer::start().await;
            Mock::given(any())
                .respond_with(ResponseTemplate::new(401).set_body_string(body))
                .mount(&server)
                .await;
            let client = LlmClient::new(LlmConfig {
                base_url: server.uri(),
                api_key: KEY.to_string(),
                ..LlmConfig::default()
            });
            let messages = [Message::user("hi")];
            if stream {
                client
                    .chat_stream(&messages, None)
                    .await
                    .err()
                    .expect("the endpoint refused the stream")
            } else {
                client
                    .chat(&messages, None)
                    .await
                    .expect_err("the endpoint refused the turn")
            }
        }

        #[tokio::test]
        async fn a_refusal_never_carries_the_key_it_was_sent() {
            for body in echoes() {
                for stream in [false, true] {
                    let error = refused(&body, stream).await;
                    let LlmError::Api { status, message } = &error else {
                        panic!("{error:?}");
                    };
                    let shown = error.to_string();

                    assert_eq!(*status, 401);
                    for leaked in [KEY, "wxyz", "sk-proj-AbCd"] {
                        assert!(!message.contains(leaked), "{body} -> {message}");
                        assert!(!shown.contains(leaked), "{body} -> {shown}");
                    }
                    assert!(message.contains(crate::secret::REDACTED), "{message}");
                }
            }
        }

        #[tokio::test]
        async fn a_refusal_without_the_key_is_reported_as_the_endpoint_sent_it() {
            let body = r#"{"error":{"message":"The model `gpt-9` does not exist"}}"#;

            let error = refused(body, false).await;

            assert!(
                matches!(&error, LlmError::Api { status: 401, message } if message == body),
                "{error:?}"
            );
        }
    }

    mod tenant {
        use super::*;
        use serde_json::json;
        use wiremock::matchers::any;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        const KEY: &str = "sk-tenant-AbCdEfGh1234567890wxyz";

        fn client(base_url: &str) -> LlmClient {
            LlmClient::new(LlmConfig {
                base_url: base_url.to_string(),
                api_key: KEY.to_string(),
                trust: Trust::Tenant,
                ..LlmConfig::default()
            })
        }

        async fn refusal(response: ResponseTemplate) -> LlmError {
            let server = MockServer::start().await;
            Mock::given(any())
                .respond_with(response)
                .mount(&server)
                .await;
            client(&server.uri())
                .chat(&[Message::user("hi")], None)
                .await
                .expect_err("the endpoint refused the turn")
        }

        fn reported(error: &LlmError) -> (u16, String) {
            let LlmError::Api { status, message } = error else {
                panic!("{error:?}");
            };
            let envelope: serde_json::Value =
                serde_json::from_str(message).expect("an error envelope");
            let text = envelope["error"]["message"]
                .as_str()
                .expect("an error message")
                .to_string();
            assert_eq!(
                envelope,
                json!({ "error": { "message": text } }),
                "{message}"
            );
            (*status, text)
        }

        #[tokio::test]
        async fn a_tenant_endpoint_is_never_sent_to_a_metadata_address() {
            for base_url in [
                "http://169.254.169.254/latest",
                "http://169.254.170.2/v1",
                "http://[fe80::1]:8080/v1",
                "http://[fd00:ec2::254]/v1",
                "http://[::ffff:169.254.169.254]/v1",
                "http://metadata.google.internal/computeMetadata/v1",
                "http://METADATA.GOOGLE.INTERNAL./v1",
            ] {
                let refused = client(base_url).chat(&[Message::user("hi")], None).await;

                assert!(
                    matches!(&refused, Err(LlmError::InvalidConfig(message)) if message.contains("metadata")),
                    "{base_url}: {refused:?}"
                );
            }
        }

        #[tokio::test]
        async fn a_tenant_endpoint_is_never_redirected_to_a_metadata_address() {
            let server = MockServer::start().await;
            Mock::given(any())
                .respond_with(
                    ResponseTemplate::new(307)
                        .insert_header("location", "http://169.254.169.254/latest/meta-data/"),
                )
                .expect(1)
                .mount(&server)
                .await;

            let refused = client(&server.uri())
                .chat(&[Message::user("hi")], None)
                .await
                .expect_err("a redirect is refused");

            assert_eq!(reported(&refused), (307, "Temporary Redirect".to_string()));
            server.verify().await;
        }

        #[tokio::test]
        async fn a_tenant_endpoint_is_never_followed_through_a_redirect() {
            let elsewhere = MockServer::start().await;
            Mock::given(any())
                .respond_with(ResponseTemplate::new(200))
                .expect(0)
                .mount(&elsewhere)
                .await;
            let server = MockServer::start().await;
            Mock::given(any())
                .respond_with(
                    ResponseTemplate::new(307)
                        .insert_header("location", format!("{}/admin", elsewhere.uri())),
                )
                .expect(1)
                .mount(&server)
                .await;

            let refused = client(&server.uri())
                .chat(&[Message::user("hi")], None)
                .await
                .expect_err("a redirect is refused");

            assert_eq!(reported(&refused), (307, "Temporary Redirect".to_string()));
            server.verify().await;
            elsewhere.verify().await;
        }

        async fn proxied(trust: Trust) -> (usize, usize) {
            let proxy = MockServer::start().await;
            Mock::given(any())
                .respond_with(ResponseTemplate::new(200))
                .mount(&proxy)
                .await;
            let endpoint = MockServer::start().await;
            Mock::given(any())
                .respond_with(ResponseTemplate::new(200))
                .mount(&endpoint)
                .await;
            let builder =
                Client::builder().proxy(reqwest::Proxy::all(proxy.uri()).expect("a proxy URL"));
            let client = guarded(builder, trust).build().expect("a client");

            let sent = client.post(endpoint.uri()).send().await;

            assert!(sent.is_ok(), "{trust:?}: {sent:?}");
            (reached(&proxy).await, reached(&endpoint).await)
        }

        async fn reached(server: &MockServer) -> usize {
            server.received_requests().await.unwrap_or_default().len()
        }

        #[tokio::test]
        async fn a_tenant_endpoint_is_never_sent_through_a_proxy() {
            assert_eq!(
                proxied(Trust::Operator).await,
                (1, 0),
                "the operator's client goes through the proxy it was given"
            );
            assert_eq!(
                proxied(Trust::Tenant).await,
                (0, 1),
                "a proxy resolves the tenant's host itself, past the metadata guard"
            );
        }

        #[tokio::test]
        async fn a_tenant_endpoint_still_reaches_loopback() {
            let server = MockServer::start().await;
            Mock::given(any())
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "id": "completion",
                    "object": "chat.completion",
                    "created": 0,
                    "model": "test",
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "hi"},
                        "finish_reason": "stop"
                    }]
                })))
                .expect(1)
                .mount(&server)
                .await;
            let localhost = server.uri().replace("127.0.0.1", "localhost");

            let answered = client(&localhost).chat(&[Message::user("hi")], None).await;

            assert!(answered.is_ok(), "{answered:?}");
            server.verify().await;
        }

        #[tokio::test]
        async fn a_tenant_refusal_reports_only_its_status_and_error_message() {
            let error = refusal(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "message": "The model `gpt-9` does not exist",
                    "type": "invalid_request_error",
                },
                "reflected": "ami-id: i-0123456789abcdef0",
            })))
            .await;

            assert_eq!(
                reported(&error),
                (400, "The model `gpt-9` does not exist".to_string())
            );
            assert!(!error.to_string().contains("ami-id"), "{error}");
        }

        #[tokio::test]
        async fn a_tenant_refusal_without_an_error_message_reports_its_status_alone() {
            for body in [
                "<html><body>instance-id: i-0123456789abcdef0</body></html>",
                r#"{"Code":"Success","AccessKeyId":"ASIAEXAMPLE"}"#,
                "",
            ] {
                let error = refusal(ResponseTemplate::new(502).set_body_string(body)).await;

                assert_eq!(reported(&error), (502, "Bad Gateway".to_string()), "{body}");
            }
        }

        #[tokio::test]
        async fn a_tenant_refusal_is_capped_and_never_carries_the_key() {
            let long = format!("{}{KEY} sk-tenant-****wxyz", "x".repeat(2_000));
            let error = refusal(
                ResponseTemplate::new(401)
                    .set_body_json(json!({ "error": { "message": format!("bad key {KEY}") } })),
            )
            .await;
            let (_, message) = reported(&error);
            assert!(!message.contains(KEY), "{message}");

            let error = refusal(
                ResponseTemplate::new(400).set_body_json(json!({ "error": { "message": long } })),
            )
            .await;
            let (_, message) = reported(&error);

            assert_eq!(message.chars().count(), REPORTED_LIMIT + 1);
            assert!(message.ends_with(ELLIPSIS), "{message}");
            assert!(!message.contains("wxyz"), "{message}");
        }

        #[tokio::test]
        async fn a_tenant_refusal_still_says_when_a_model_takes_no_tools() {
            for body in [
                json!({ "error": "llava:7b does not support tools" }),
                json!({ "error": { "message": "llava:7b does not support tools" } }),
            ] {
                let error = refusal(ResponseTemplate::new(400).set_body_json(body)).await;

                assert!(error.unsupported_tools(), "{error:?}");
            }
        }
    }

    mod shape {
        use super::*;
        use crate::llm::{Dialect, TEMPLATE_STOPS};
        use serde_json::{Value, json};
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

        const TEMPERATURE: f32 = 1.4;
        const RESERVED: u32 = 321;

        struct Without(&'static str);

        impl Match for Without {
            fn matches(&self, request: &Request) -> bool {
                serde_json::from_slice::<Value>(&request.body)
                    .is_ok_and(|body| body.get(self.0).is_none())
            }
        }

        fn completion() -> ResponseTemplate {
            ResponseTemplate::new(200).set_body_json(json!({
                "id": "completion",
                "object": "chat.completion",
                "created": 0,
                "model": "test",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "hi"},
                    "finish_reason": "stop"
                }]
            }))
        }

        fn streamed() -> ResponseTemplate {
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
                )
        }

        fn stops() -> Vec<String> {
            TEMPLATE_STOPS
                .iter()
                .map(|stop| (*stop).to_string())
                .chain(["User:", " ", "Human:", "###", "END", "STOP"].map(String::from))
                .collect()
        }

        fn client(server: &MockServer, dialect: Dialect) -> LlmClient {
            LlmClient::new(LlmConfig {
                base_url: server.uri(),
                temperature: TEMPERATURE,
                max_tokens: RESERVED,
                dialect,
                ..LlmConfig::default()
            })
            .with_stop(stops())
            .with_ollama_context("model", 8192)
        }

        async fn sent(server: &MockServer, dialect: Dialect, model: &str) -> Value {
            let answered = client(server, dialect)
                .chat_with_model(model, &[Message::user("hi")], None)
                .await;
            assert!(answered.is_ok(), "{dialect:?} {model}: {answered:?}");
            let requests = server.received_requests().await.unwrap_or_default();
            let request = requests.last().expect("a request reached the endpoint");
            serde_json::from_slice(&request.body).expect("a JSON body")
        }

        fn stop_count(body: &Value) -> usize {
            body.get("stop")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        }

        #[tokio::test]
        async fn openai_is_sent_at_most_four_stops_and_no_template_tokens() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_partial_json(json!({
                    "max_tokens": RESERVED,
                    "stop": ["User:", " ", "Human:", "###"],
                })))
                .and(Without("max_completion_tokens"))
                .and(Without("num_ctx"))
                .respond_with(completion())
                .expect(1)
                .mount(&server)
                .await;

            let body = sent(&server, Dialect::OpenAI, "gpt-4o").await;

            assert_eq!(stop_count(&body), 4, "{body}");
            assert!(body.get("temperature").is_some(), "{body}");
            server.verify().await;
        }

        #[tokio::test]
        async fn an_openai_reasoning_model_is_sent_no_stop_no_temperature_and_a_completion_budget()
        {
            for model in ["o3-mini", "o1", "o4-mini", "gpt-5"] {
                let server = MockServer::start().await;
                Mock::given(method("POST"))
                    .and(path("/chat/completions"))
                    .and(body_partial_json(
                        json!({ "max_completion_tokens": RESERVED }),
                    ))
                    .and(Without("max_tokens"))
                    .and(Without("temperature"))
                    .and(Without("stop"))
                    .respond_with(completion())
                    .expect(1)
                    .mount(&server)
                    .await;

                let body = sent(&server, Dialect::OpenAI, model).await;

                assert_eq!(stop_count(&body), 0, "{model}: {body}");
                server.verify().await;
            }
        }

        #[tokio::test]
        async fn an_openai_reasoning_model_streams_under_the_same_shape() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_partial_json(json!({
                    "max_completion_tokens": RESERVED,
                    "stream": true,
                })))
                .and(Without("max_tokens"))
                .and(Without("temperature"))
                .and(Without("stop"))
                .respond_with(streamed())
                .expect(1)
                .mount(&server)
                .await;

            let mut stream = client(&server, Dialect::OpenAI)
                .chat_stream_with_model("o3", &[Message::user("hi")], None)
                .await
                .expect("the stream opens");
            while let Some(chunk) = stream.next().await {
                assert!(chunk.is_ok(), "{chunk:?}");
            }
            server.verify().await;
        }

        #[tokio::test]
        async fn openai_is_sent_reasoning_effort_only_for_a_model_that_reasons() {
            for (model, expected) in [("o3", Some("high")), ("gpt-4o", None)] {
                let server = MockServer::start().await;
                Mock::given(method("POST"))
                    .respond_with(completion())
                    .mount(&server)
                    .await;
                let answered = client(&server, Dialect::OpenAI)
                    .with_reasoning(model, crate::llm::Effort::High)
                    .chat_with_model(model, &[Message::user("hi")], None)
                    .await;
                assert!(answered.is_ok(), "{answered:?}");
                let requests = server.received_requests().await.unwrap_or_default();
                let body: Value = serde_json::from_slice(&requests[0].body).expect("a JSON body");

                assert_eq!(
                    body.get("reasoning_effort").and_then(Value::as_str),
                    expected,
                    "{model}: {body}"
                );
            }
        }

        #[tokio::test]
        async fn anthropic_is_sent_every_non_blank_stop_and_a_temperature_of_at_most_one() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_partial_json(json!({
                    "max_tokens": RESERVED,
                    "temperature": 1.0,
                })))
                .and(Without("max_completion_tokens"))
                .and(Without("num_ctx"))
                .respond_with(completion())
                .expect(1)
                .mount(&server)
                .await;

            let body = sent(&server, Dialect::Anthropic, "claude-sonnet-4-5").await;

            assert_eq!(stop_count(&body), stops().len() - 1, "{body}");
            assert!(
                body["stop"]
                    .as_array()
                    .is_some_and(|stops| stops.iter().all(|stop| stop != " ")),
                "{body}"
            );
            server.verify().await;
        }

        #[tokio::test]
        async fn a_compatible_endpoint_is_sent_the_request_as_it_always_was() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_partial_json(json!({
                    "max_tokens": RESERVED,
                    "num_ctx": 8192,
                })))
                .and(Without("max_completion_tokens"))
                .respond_with(completion())
                .expect(1)
                .mount(&server)
                .await;

            let body = sent(&server, Dialect::Compatible, "model").await;

            assert_eq!(stop_count(&body), stops().len(), "{body}");
            assert_eq!(
                body["temperature"].as_f64().map(|value| value as f32),
                Some(TEMPERATURE),
                "{body}"
            );
            server.verify().await;
        }
    }

    use crate::llm::Effort;
    use crate::llm::provider::ProviderError;
    use crate::llm::types::{ChatRequest, FunctionCall, Message, ToolCall, Usage};
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// The line a signed-out claude 2.1.269 ends its run with. The subtype
    /// says success and the run still failed.
    const SIGNED_OUT: &str = r#"{"type":"result","subtype":"success","is_error":true,"terminal_reason":"api_error","result":"Not logged in · Please run /login"}"#;
    const REFUSAL: &str = "Not logged in \u{b7} Please run /login";

    /// What `claude --print --output-format stream-json` really emits when the
    /// host session has expired: the refusal arrives as assistant text first,
    /// and only the result that follows says it was never an answer.
    const SIGNED_OUT_ASSISTANT: &str = r#"{"type":"assistant","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-4","content":[{"type":"text","text":"Not logged in \u00b7 Please run /login"}],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":1}},"session_id":"s1"}"#;

    /// A stand-in for an agent CLI, so no test needs one signed in on the host.
    fn fake(directory: &TempDir, script: &str) -> PathBuf {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let path = directory.path().join("agent");
        let mut file = std::fs::File::create(&path).expect("the fake agent");
        write!(file, "#!/bin/sh\n{script}\n").expect("the fake agent body");
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("the fake agent to be executable");
        path
    }

    fn agent_client(executable: PathBuf) -> LlmClient {
        let settings = CliSettings::default()
            .with_executable(executable)
            .with_timeout(Duration::from_secs(20));
        LlmClient::new(
            LlmConfig::default().with_backend(LlmBackend::cli(AgentKind::Claude, settings)),
        )
    }

    fn agent_events(events: Vec<Result<AgentEvent, ProviderError>>) -> AgentStream {
        Box::pin(futures::stream::iter(events))
    }

    async fn collected(
        stream: impl Stream<Item = Result<ChatStreamChunk, LlmError>>,
    ) -> (Vec<ChatStreamChunk>, Option<LlmError>) {
        let mut stream = Box::pin(stream);
        let mut delivered = Vec::new();
        let mut failure = None;
        while let Some(item) = stream.next().await {
            match item {
                Ok(chunk) => delivered.push(chunk),
                Err(error) => failure = Some(error),
            }
        }
        (delivered, failure)
    }

    fn spoken(chunks: &[ChatStreamChunk]) -> String {
        chunks
            .iter()
            .filter_map(|chunk| chunk.choices.first()?.delta.content.clone())
            .collect()
    }

    fn reasoned(chunks: &[ChatStreamChunk]) -> String {
        chunks
            .iter()
            .filter_map(|chunk| chunk.choices.first()?.delta.reasoning_content.clone())
            .collect()
    }

    fn reasons(chunks: &[ChatStreamChunk]) -> Vec<String> {
        chunks
            .iter()
            .filter_map(|chunk| chunk.choices.first()?.finish_reason.clone())
            .collect()
    }

    async fn stream_turn(
        client: &LlmClient,
        tools: Option<&[ToolDefinition]>,
    ) -> Result<ChatStream, LlmError> {
        client
            .chat_stream_with_options(
                "sonnet",
                &[Message::user("What does a.rs do?")],
                tools,
                RequestOptions { reserved: 512 },
            )
            .await
    }

    #[test]
    fn classifies_explicit_tool_capability_rejections() {
        for (status, message) in [
            (400, r#"{"error":"llava:7b does not support tools"}"#),
            (
                500,
                r#"{"error":{"message":"Ollama_chatException - llava:7b does not support tools"}}"#,
            ),
        ] {
            assert!(
                LlmError::Api {
                    status,
                    message: message.into()
                }
                .unsupported_tools()
            );
        }
    }

    #[test]
    fn preserves_authentication_transport_and_schema_failures() {
        for (status, message) in [
            (401, r#"{"error":{"message":"does not support tools"}}"#),
            (403, r#"{"error":{"message":"does not support tools"}}"#),
            (429, r#"{"error":{"message":"does not support tools"}}"#),
            (500, r#"{"error":{"message":"Connection refused"}}"#),
            (400, r#"{"error":{"message":"Invalid tool schema"}}"#),
            (500, "does not support tools"),
        ] {
            assert!(
                !LlmError::Api {
                    status,
                    message: message.into()
                }
                .unsupported_tools()
            );
        }
        assert!(!LlmError::Stream("does not support tools".into()).unsupported_tools());
    }

    #[test]
    fn test_llm_config_default_values() {
        let config = LlmConfig::default();

        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(config.api_key, "");
        assert_eq!(config.default_model, "gpt-4");
        assert!((config.temperature - 0.7).abs() < f32::EPSILON);
        assert_eq!(config.max_tokens, 4096);
    }

    #[test]
    fn test_llm_config_custom_values() {
        let config = LlmConfig {
            base_url: "https://custom.api.com/v1".to_string(),
            api_key: "sk-test-key-123".to_string(),
            default_model: "gpt-3.5-turbo".to_string(),
            temperature: 0.5,
            max_tokens: 2048,
            ..LlmConfig::default()
        };

        assert_eq!(config.base_url, "https://custom.api.com/v1");
        assert_eq!(config.api_key, "sk-test-key-123");
        assert_eq!(config.default_model, "gpt-3.5-turbo");
        assert!((config.temperature - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.max_tokens, 2048);
    }

    #[test]
    fn test_llm_config_clone() {
        let config = LlmConfig {
            base_url: "https://test.api.com".to_string(),
            api_key: "test-key".to_string(),
            default_model: "test-model".to_string(),
            temperature: 0.9,
            max_tokens: 1000,
            ..LlmConfig::default()
        };

        let cloned = config.clone();
        assert_eq!(cloned.base_url, config.base_url);
        assert_eq!(cloned.api_key, config.api_key);
        assert_eq!(cloned.default_model, config.default_model);
        assert!((cloned.temperature - config.temperature).abs() < f32::EPSILON);
        assert_eq!(cloned.max_tokens, config.max_tokens);
    }

    /// A default config's `api_key` is the empty string, so asserting that its
    /// debug output merely *mentions* `api_key` is satisfied by a derived
    /// `Debug` -- the exact defect the hand-written one exists to prevent. The
    /// key here is a real one, and what is asserted is that its value is absent.
    #[test]
    fn test_llm_config_debug() {
        const KEY: &str = "sk-test-3f8a1c9e04b27d65";
        let config = LlmConfig {
            api_key: KEY.to_string(),
            ..LlmConfig::default()
        };
        let debug_str = format!("{:?}", config);

        assert!(debug_str.contains("LlmConfig"));
        assert!(debug_str.contains("base_url"));
        assert!(debug_str.contains("default_model"));
        assert!(
            debug_str.contains("api_key"),
            "the field should still be named, so its redaction is visible: {debug_str}"
        );
        assert!(
            !debug_str.contains(KEY),
            "the provider key reached a debug line: {debug_str}"
        );
    }

    #[test]
    fn test_llm_client_creation() {
        let config = LlmConfig::default();
        let client = LlmClient::new(config.clone());

        assert_eq!(client.config().base_url, config.base_url);
        assert_eq!(client.config().api_key, config.api_key);
        assert_eq!(client.config().default_model, config.default_model);
    }

    #[test]
    fn test_llm_client_with_custom_config() {
        let config = LlmConfig {
            base_url: "https://custom.openai.com/v1".to_string(),
            api_key: "sk-custom-key".to_string(),
            default_model: "gpt-4-turbo".to_string(),
            temperature: 0.3,
            max_tokens: 8192,
            ..LlmConfig::default()
        };

        let client = LlmClient::new(config);

        assert_eq!(client.config().base_url, "https://custom.openai.com/v1");
        assert_eq!(client.config().api_key, "sk-custom-key");
        assert_eq!(client.config().default_model, "gpt-4-turbo");
        assert!((client.config().temperature - 0.3).abs() < f32::EPSILON);
        assert_eq!(client.config().max_tokens, 8192);
    }

    #[test]
    fn test_llm_client_clone() {
        let config = LlmConfig {
            base_url: "https://test.api.com".to_string(),
            api_key: "test-key".to_string(),
            default_model: "test-model".to_string(),
            temperature: 0.6,
            max_tokens: 512,
            ..LlmConfig::default()
        };

        let client = LlmClient::new(config);
        let cloned = client.clone();

        assert_eq!(cloned.config().base_url, client.config().base_url);
        assert_eq!(cloned.config().api_key, client.config().api_key);
    }

    #[test]
    fn test_llm_client_debug() {
        const KEY: &str = "sk-test-6b02da97e15c4f38";
        let config = LlmConfig {
            api_key: KEY.to_string(),
            ..LlmConfig::default()
        };
        let client = LlmClient::new(config);
        let debug_str = format!("{:?}", client);

        assert!(debug_str.contains("LlmClient"));
        assert!(debug_str.contains("config"));
        // Every worker logs the client, and the config travels inside it.
        assert!(
            !debug_str.contains(KEY),
            "the provider key reached a debug line through the client: {debug_str}"
        );
    }

    #[test]
    fn test_llm_error_api_display() {
        let error = LlmError::Api {
            status: 401,
            message: "Unauthorized".to_string(),
        };

        let display = format!("{}", error);
        assert!(display.contains("401"));
        assert!(display.contains("Unauthorized"));
        assert!(display.contains("API error"));
    }

    #[test]
    fn test_llm_error_stream_display() {
        let error = LlmError::Stream("Connection reset".to_string());
        let display = format!("{}", error);

        assert!(display.contains("Stream error"));
        assert!(display.contains("Connection reset"));
    }

    #[test]
    fn test_llm_error_json_from() {
        let json_error = serde_json::from_str::<serde_json::Value>("invalid json");
        assert!(json_error.is_err());

        let llm_error: LlmError = json_error.unwrap_err().into();
        let display = format!("{}", llm_error);
        assert!(display.contains("JSON error"));
    }

    #[test]
    fn test_llm_error_api_various_status_codes() {
        let test_cases = vec![
            (400, "Bad Request"),
            (401, "Unauthorized"),
            (403, "Forbidden"),
            (404, "Not Found"),
            (429, "Rate Limited"),
            (500, "Internal Server Error"),
            (503, "Service Unavailable"),
        ];

        for (status, message) in test_cases {
            let error = LlmError::Api {
                status,
                message: message.to_string(),
            };
            let display = format!("{}", error);
            assert!(display.contains(&status.to_string()));
            assert!(display.contains(message));
        }
    }

    #[test]
    fn test_llm_error_debug() {
        let error = LlmError::Api {
            status: 500,
            message: "Server error".to_string(),
        };
        let debug_str = format!("{:?}", error);

        assert!(debug_str.contains("Api"));
        assert!(debug_str.contains("500"));
    }

    #[test]
    fn test_llm_error_stream_empty_message() {
        let error = LlmError::Stream(String::new());
        let display = format!("{}", error);
        assert!(display.contains("Stream error"));
    }

    #[test]
    fn test_llm_error_api_empty_message() {
        let error = LlmError::Api {
            status: 500,
            message: String::new(),
        };
        let display = format!("{}", error);
        assert!(display.contains("500"));
        assert!(display.contains("API error"));
    }

    #[test]
    fn reasoning_effort_is_model_bound_and_uses_the_resolved_level() {
        let messages = [Message::user("Hi")];
        let client = LlmClient::new(LlmConfig::default()).with_reasoning("thinker", Effort::High);
        let enabled = client
            .request(ChatRequest {
                model: "thinker",
                messages: &messages,
                tools: None,
                tool_choice: None,
                temperature: None,
                max_tokens: Some(4096),
                max_completion_tokens: None,
                stream: None,
                stop: None,
            })
            .unwrap();
        assert_eq!(enabled["reasoning_effort"], "high");
        let other = client
            .request(ChatRequest {
                model: "other",
                messages: &messages,
                tools: None,
                tool_choice: None,
                temperature: None,
                max_tokens: Some(4096),
                max_completion_tokens: None,
                stream: None,
                stop: None,
            })
            .unwrap();
        assert!(other.get("reasoning_effort").is_none());
        let compact = client
            .without_reasoning()
            .request(ChatRequest {
                model: "thinker",
                messages: &messages,
                tools: None,
                tool_choice: None,
                temperature: None,
                max_tokens: Some(4096),
                max_completion_tokens: None,
                stream: None,
                stop: None,
            })
            .unwrap();
        assert!(compact.get("reasoning_effort").is_none());
    }

    #[test]
    fn a_public_https_host_is_accepted() {
        assert!(validate_outbound_url("https://api.openai.com/v1/chat/completions").is_ok());
    }

    #[test]
    fn a_non_http_scheme_is_refused() {
        let error = validate_outbound_url("file:///etc/passwd").unwrap_err();
        assert!(
            matches!(&error, LlmError::InvalidConfig(message) if message.contains("http or https")),
            "the refusal has to name the scheme rule: {error}"
        );
    }

    #[test]
    fn credentials_in_the_url_are_refused() {
        let error = validate_outbound_url("https://user:pass@api.openai.com/v1").unwrap_err();
        assert!(
            matches!(&error, LlmError::InvalidConfig(message) if message.contains("userinfo")),
            "the refusal has to name the userinfo rule: {error}"
        );
    }

    #[test]
    fn the_hosts_a_self_hosted_zone_runs_on_are_accepted() {
        for base_url in [
            "http://localhost:4000",
            "http://127.0.0.1:11434",
            "http://192.168.1.50:4000",
            "http://host.docker.internal:11434",
            "http://litellm:4000",
            "http://[::1]:4000",
        ] {
            assert!(
                validate_outbound_url(base_url).is_ok(),
                "{base_url} is a supported way to reach a self-hosted model server"
            );
        }
    }

    #[test]
    fn a_relative_url_is_refused() {
        let error = validate_outbound_url("/v1/chat/completions").unwrap_err();
        assert!(
            matches!(&error, LlmError::InvalidConfig(message) if message.contains("absolute URL")),
            "the refusal has to name the absolute-URL rule: {error}"
        );
    }

    /// The request has to be made against the URL the guard returned, not the
    /// string it was handed, or the check and the send can disagree.
    #[test]
    fn the_checked_url_is_the_one_handed_back() {
        let checked = validate_outbound_url("http://litellm:4000/v1/chat/completions")
            .expect("a compose service host is reachable");
        assert_eq!(checked.as_str(), "http://litellm:4000/v1/chat/completions");
        assert_eq!(checked.host_str(), Some("litellm"));
    }

    #[test]
    fn completions_come_from_the_endpoint_unless_a_backend_says_otherwise() {
        assert!(matches!(LlmConfig::default().backend, LlmBackend::Http));
        assert!(matches!(
            LlmConfig::default()
                .with_backend(LlmBackend::cli(AgentKind::Codex, CliSettings::default()))
                .backend,
            LlmBackend::Cli {
                agent: AgentKind::Codex,
                ..
            }
        ));
    }

    #[test]
    fn a_turns_toolset_reaches_the_agent_the_backend_spawns() {
        let toolset = Toolset::new(
            "http://127.0.0.1:8421/mcp",
            "zone-turn-notarealtoken",
            ["read_file"],
        );

        let client = LlmClient::new(
            LlmConfig::default()
                .with_backend(LlmBackend::cli(AgentKind::Claude, CliSettings::default())),
        )
        .with_toolset(toolset, BuiltinTools::Withheld);

        let LlmBackend::Cli { settings, .. } = &client.config().backend else {
            panic!("the backend stayed on the endpoint");
        };
        let attached = settings.toolset.as_ref().expect("the turn's toolset");
        assert_eq!(attached.endpoint, "http://127.0.0.1:8421/mcp");
        assert_eq!(attached.tools, ["read_file"]);
        assert_eq!(settings.builtin_tools, BuiltinTools::Withheld);
    }

    /// An HTTP backend spawns nothing to configure and carries its tools in
    /// the request itself, so there is nothing here for it to lose.
    #[test]
    fn a_toolset_leaves_an_http_backend_exactly_as_it_was() {
        let client = LlmClient::new(LlmConfig::default()).with_toolset(
            Toolset::new("http://127.0.0.1:8421/mcp", "token", ["read_file"]),
            BuiltinTools::Granted,
        );

        assert!(matches!(client.config().backend, LlmBackend::Http));
    }

    /// The builder replaces what a turn decides and nothing else: the executable
    /// the operator configured is still the one that runs.
    #[test]
    fn attaching_a_toolset_keeps_the_rest_of_the_settings() {
        let client = LlmClient::new(LlmConfig::default().with_backend(LlmBackend::cli(
            AgentKind::Claude,
            CliSettings::default().with_executable("/opt/bin/claude"),
        )))
        .with_toolset(
            Toolset::new("http://127.0.0.1:8421/mcp", "token", ["read_file"]),
            BuiltinTools::Granted,
        );

        let LlmBackend::Cli { settings, .. } = &client.config().backend else {
            panic!("the backend stayed on the endpoint");
        };
        assert_eq!(
            settings.executable,
            Some(std::path::PathBuf::from("/opt/bin/claude"))
        );
        assert_eq!(settings.builtin_tools, BuiltinTools::Granted);
    }

    #[tokio::test]
    async fn an_agents_successful_ending_becomes_the_one_the_loop_accepts() {
        let events = agent_events(vec![
            Ok(AgentEvent::Text("The suite passes.".to_string())),
            Ok(AgentEvent::Usage(Usage {
                prompt_tokens: 40,
                completion_tokens: 8,
                total_tokens: 48,
            })),
            Ok(AgentEvent::Finished {
                finish_reason: Some("success".to_string()),
            }),
        ]);

        let (delivered, failure) = collected(chunks(events, "claude".to_string())).await;

        assert!(failure.is_none(), "a finished turn failed: {failure:?}");
        assert_eq!(spoken(&delivered), "The suite passes.");
        assert_eq!(
            delivered
                .iter()
                .find_map(|chunk| chunk.usage.as_ref())
                .map(|usage| usage.total_tokens),
            Some(48)
        );
        assert_eq!(
            reasons(&delivered),
            [STOP],
            "the agent's own word would spin the loop to its iteration limit"
        );
    }

    #[tokio::test]
    async fn an_agents_tool_use_is_shown_as_reasoning_rather_than_replayed() {
        let events = agent_events(vec![
            Ok(AgentEvent::Tool(ToolCall {
                id: "toolu_01".to_string(),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: "Read".to_string(),
                    arguments: r#"{"file_path":"/w/a.rs"}"#.to_string(),
                },
            })),
            Ok(AgentEvent::Finished {
                finish_reason: Some("success".to_string()),
            }),
        ]);

        let (delivered, failure) = collected(chunks(events, "claude".to_string())).await;

        assert!(failure.is_none(), "a finished turn failed: {failure:?}");
        assert!(
            reasoned(&delivered).contains("Read"),
            "the reader was shown nothing for the work the agent did"
        );
        assert!(
            delivered.iter().all(|chunk| chunk
                .choices
                .iter()
                .all(|choice| choice.delta.tool_calls.is_none())),
            "zone would run the agent's own tool a second time"
        );
    }

    #[tokio::test]
    async fn a_refusing_agent_ends_the_stream_in_its_own_words() {
        let events = agent_events(vec![Ok(AgentEvent::Failed(REFUSAL.to_string()))]);

        let (delivered, failure) = collected(chunks(events, "claude".to_string())).await;

        let failure = failure.expect("a refused turn to fail");
        assert!(
            failure.to_string().contains(REFUSAL),
            "lost the agent's wording: {failure}"
        );
        assert!(
            delivered.is_empty(),
            "a refusal was delivered as an answer: {delivered:?}"
        );
    }

    #[tokio::test]
    async fn a_host_agent_answers_a_streaming_turn() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Looking now. "}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/w/a.rs"}}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"It is empty."}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":9,"cache_read_input_tokens":1600,"output_tokens":24}}'
"#;
        let client = agent_client(fake(&directory, script));

        let stream = stream_turn(&client, None).await.expect("a running agent");
        let (delivered, failure) = collected(stream).await;

        assert!(failure.is_none(), "the turn failed: {failure:?}");
        assert_eq!(spoken(&delivered), "Looking now. It is empty.");
        assert!(reasoned(&delivered).contains("Read"));
        assert_eq!(reasons(&delivered), [STOP]);
        assert_eq!(
            delivered
                .iter()
                .find_map(|chunk| chunk.usage.as_ref())
                .map(|usage| usage.prompt_tokens),
            Some(9 + 1600)
        );
    }

    #[tokio::test]
    async fn a_signed_out_host_agent_fails_the_turn_rather_than_answering_it() {
        let directory = TempDir::new().expect("a temporary directory");
        let client = agent_client(fake(&directory, &format!("printf '%s\\n' '{SIGNED_OUT}'")));

        let stream = stream_turn(&client, None).await.expect("a running agent");
        let (delivered, failure) = collected(stream).await;

        let failure = failure.expect("a signed-out agent to fail the turn");
        assert!(
            failure.to_string().contains(REFUSAL),
            "lost the agent's wording: {failure}"
        );
        assert!(
            !spoken(&delivered).contains("Not logged in"),
            "the refusal was streamed as the answer"
        );
        assert!(
            reasons(&delivered).is_empty(),
            "a refused turn was ended as a finished one"
        );
    }

    #[tokio::test]
    async fn a_refusal_spoken_before_it_is_declared_still_fails_the_turn() {
        let directory = TempDir::new().expect("a temporary directory");
        let client = agent_client(fake(
            &directory,
            &format!("printf '%s\\n' '{SIGNED_OUT_ASSISTANT}' '{SIGNED_OUT}'"),
        ));

        let stream = stream_turn(&client, None).await.expect("a running agent");
        let (delivered, failure) = collected(stream).await;

        let failure = failure.expect("a signed-out agent to fail the turn");
        assert!(
            failure.to_string().contains(REFUSAL),
            "lost the agent's wording: {failure}"
        );
        assert!(
            reasons(&delivered).is_empty(),
            "a turn that spoke before it failed was still ended as a finished one"
        );
    }

    #[tokio::test]
    async fn tools_offered_to_a_cli_backend_are_refused_rather_than_dropped() {
        let directory = TempDir::new().expect("a temporary directory");
        let client = agent_client(fake(&directory, "exit 1"));
        let tools = [ToolDefinition::function(
            "read_file",
            "Read a file",
            serde_json::json!({}),
        )];

        let Err(streaming) = stream_turn(&client, Some(&tools)).await else {
            panic!("a streaming turn offered zone's tools to an agent that cannot call them");
        };
        let buffered = client
            .chat_with_options(
                "sonnet",
                &[Message::user("hi")],
                Some(&tools),
                RequestOptions { reserved: 512 },
            )
            .await
            .expect_err("a refusal");

        for error in [streaming, buffered] {
            assert!(
                matches!(error, LlmError::InvalidConfig(_)),
                "expected a rejected request, got {error:?}"
            );
            assert!(
                error.to_string().contains("read_file"),
                "the refusal has to name the tools it could not serve: {error}"
            );
        }
    }

    #[tokio::test]
    async fn a_buffered_turn_reaches_the_host_agent_too() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Two."}]}}'
echo '{"type":"result","subtype":"success","is_error":false}'
"#;
        let client = agent_client(fake(&directory, script));

        let response = client
            .chat_with_options(
                "sonnet",
                &[Message::user("one plus one?")],
                None,
                RequestOptions { reserved: 512 },
            )
            .await
            .expect("an answer");

        let choice = response.choices.first().expect("one choice");
        assert_eq!(choice.message.content.as_deref(), Some("Two."));
        assert_eq!(choice.finish_reason.as_deref(), Some(STOP));
    }
}
