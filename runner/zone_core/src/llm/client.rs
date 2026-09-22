//! LLM client for OpenAI-compatible APIs

use futures::{Stream, StreamExt};
use reqwest::{Client, Url};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::runtime;

use super::provider::{
    AgentEvent, AgentKind, AgentStream, CliProvider, CliSettings, Completion, CompletionProvider,
    CompletionRequest,
};
use super::types::{
    ChatRequest, ChatResponse, ChatStreamChunk, Choice, Message, StreamChoice, StreamDelta,
    ToolDefinition,
};

fn pool() -> Client {
    Client::builder()
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(10))
        .tcp_nodelay(true)
        .build()
        .unwrap_or_else(|_| Client::new())
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
static HTTP: LazyLock<Mutex<HashMap<runtime::Id, Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn http() -> Client {
    let Ok(runtime) = runtime::Handle::try_current() else {
        return pool();
    };
    let mut pools = HTTP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    pools.entry(runtime.id()).or_insert_with(pool).clone()
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

/// OpenAI's word for a turn that ended normally.
///
/// Claude reports `success` and codex `completed`. The agent loop takes a
/// reply as final only on `stop`, so a turn carrying the agent's own word
/// would never be accepted and would run to the iteration limit instead.
const STOP: &str = "stop";

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
            client: http(),
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

    fn request(&self, request: ChatRequest<'_>) -> Result<serde_json::Value, LlmError> {
        let mut body = serde_json::to_value(&request)?;
        if let Some((model, limit)) = &self.ollama
            && model == request.model
        {
            body["num_ctx"] = (*limit).into();
        }
        if let Some((model, effort)) = &self.reasoning
            && model == request.model
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
        Ok(self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await?)
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

        let request = ChatRequest {
            model,
            messages,
            tools,
            tool_choice: None,
            temperature: Some(self.config.temperature),
            max_tokens: Some(options.reserved),
            stream: Some(false),
            stop: (!self.stop.is_empty()).then_some(self.stop.as_slice()),
        };

        let url = format!("{}/chat/completions", self.config.base_url);

        let response = self.send(&url, &self.request(request)?).await?;

        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status: status.as_u16(),
                message,
            });
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

        let request = ChatRequest {
            model,
            messages,
            tools,
            tool_choice: None,
            temperature: Some(self.config.temperature),
            max_tokens: Some(options.reserved),
            stream: Some(true),
            stop: (!self.stop.is_empty()).then_some(self.stop.as_slice()),
        };

        let url = format!("{}/chat/completions", self.config.base_url);

        let response = self.send(&url, &self.request(request)?).await?;

        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status: status.as_u16(),
                message,
            });
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
