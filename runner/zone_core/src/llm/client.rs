//! LLM client for OpenAI-compatible APIs

use reqwest::{Client, Url};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::runtime;

use super::types::{ChatRequest, ChatResponse, ChatStreamChunk, Message, ToolDefinition};

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

/// Configuration for the LLM client
#[derive(Debug, Clone)]
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
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            default_model: "gpt-4".to_string(),
            temperature: 0.7,
            max_tokens: 4096,
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

    fn validate_outbound_url(&self, url: &str) -> Result<(), LlmError> {
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

        // No private-network rule here on purpose. Every caller passes an
        // operator-configured host, and a self-hosted Zone points at loopback,
        // a LAN address or a compose service name. The check belongs where a
        // tenant-supplied host is first accepted, against that value.
        Ok(())
    }

    async fn send(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, LlmError> {
        self.validate_outbound_url(url)?;
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
    ) -> Result<impl futures::Stream<Item = Result<ChatStreamChunk, LlmError>> + use<>, LlmError>
    {
        self.chat_stream_with_model(&self.config.default_model, messages, tools)
            .await
    }

    /// Make a streaming chat completion request with a specific model
    pub async fn chat_stream_with_model(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<impl futures::Stream<Item = Result<ChatStreamChunk, LlmError>> + use<>, LlmError>
    {
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
    ) -> Result<impl futures::Stream<Item = Result<ChatStreamChunk, LlmError>> + use<>, LlmError>
    {
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

            use futures::StreamExt;
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

        Ok(stream)
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
    use crate::llm::types::{ChatRequest, Message};

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
        };

        let cloned = config.clone();
        assert_eq!(cloned.base_url, config.base_url);
        assert_eq!(cloned.api_key, config.api_key);
        assert_eq!(cloned.default_model, config.default_model);
        assert!((cloned.temperature - config.temperature).abs() < f32::EPSILON);
        assert_eq!(cloned.max_tokens, config.max_tokens);
    }

    #[test]
    fn test_llm_config_debug() {
        let config = LlmConfig::default();
        let debug_str = format!("{:?}", config);

        assert!(debug_str.contains("LlmConfig"));
        assert!(debug_str.contains("base_url"));
        assert!(debug_str.contains("api_key"));
        assert!(debug_str.contains("default_model"));
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
        };

        let client = LlmClient::new(config);
        let cloned = client.clone();

        assert_eq!(cloned.config().base_url, client.config().base_url);
        assert_eq!(cloned.config().api_key, client.config().api_key);
    }

    #[test]
    fn test_llm_client_debug() {
        let config = LlmConfig::default();
        let client = LlmClient::new(config);
        let debug_str = format!("{:?}", client);

        assert!(debug_str.contains("LlmClient"));
        assert!(debug_str.contains("config"));
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

    fn client_for(base_url: &str) -> LlmClient {
        LlmClient::new(LlmConfig {
            base_url: base_url.to_string(),
            ..LlmConfig::default()
        })
    }

    #[test]
    fn a_public_https_host_is_accepted() {
        assert!(
            client_for("https://api.openai.com/v1")
                .validate_outbound_url("https://api.openai.com/v1/chat/completions")
                .is_ok()
        );
    }

    #[test]
    fn a_non_http_scheme_is_refused() {
        let error = client_for("file:///etc/passwd")
            .validate_outbound_url("file:///etc/passwd")
            .unwrap_err();
        assert!(
            matches!(&error, LlmError::InvalidConfig(message) if message.contains("http or https")),
            "the refusal has to name the scheme rule: {error}"
        );
    }

    #[test]
    fn credentials_in_the_url_are_refused() {
        let error = client_for("https://user:pass@api.openai.com/v1")
            .validate_outbound_url("https://user:pass@api.openai.com/v1")
            .unwrap_err();
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
                client_for(base_url).validate_outbound_url(base_url).is_ok(),
                "{base_url} is a supported way to reach a self-hosted model server"
            );
        }
    }

    #[test]
    fn a_relative_url_is_refused() {
        let error = client_for("/v1/chat/completions")
            .validate_outbound_url("/v1/chat/completions")
            .unwrap_err();
        assert!(
            matches!(&error, LlmError::InvalidConfig(message) if message.contains("absolute URL")),
            "the refusal has to name the absolute-URL rule: {error}"
        );
    }
}
