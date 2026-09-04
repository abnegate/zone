//! LLM client for OpenAI-compatible APIs

use reqwest::Client;
use thiserror::Error;

use super::types::{ChatRequest, ChatResponse, ChatStreamChunk, Message, ToolDefinition};

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

/// LLM client for making chat completion requests
#[derive(Debug, Clone)]
pub struct LlmClient {
    client: Client,
    config: LlmConfig,
}

impl LlmClient {
    /// Create a new LLM client
    pub fn new(config: LlmConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    /// Make a chat completion request
    pub async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<ChatResponse, LlmError> {
        self.chat_with_model(&self.config.default_model, messages, tools)
            .await
    }

    /// Make a chat completion request with a specific model
    pub async fn chat_with_model(
        &self,
        model: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<ChatResponse, LlmError> {
        let request = ChatRequest {
            model: model.to_string(),
            messages,
            tools,
            tool_choice: None,
            temperature: Some(self.config.temperature),
            max_tokens: Some(self.config.max_tokens),
            stream: Some(false),
        };

        let url = format!("{}/chat/completions", self.config.base_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

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
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<impl futures::Stream<Item = Result<ChatStreamChunk, LlmError>>, LlmError> {
        self.chat_stream_with_model(&self.config.default_model, messages, tools)
            .await
    }

    /// Make a streaming chat completion request with a specific model
    pub async fn chat_stream_with_model(
        &self,
        model: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<impl futures::Stream<Item = Result<ChatStreamChunk, LlmError>>, LlmError> {
        let request = ChatRequest {
            model: model.to_string(),
            messages,
            tools,
            tool_choice: None,
            temperature: Some(self.config.temperature),
            max_tokens: Some(self.config.max_tokens),
            stream: Some(true),
        };

        let url = format!("{}/chat/completions", self.config.base_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status: status.as_u16(),
                message,
            });
        }

        // Parse SSE stream
        let stream = async_stream::stream! {
            let mut bytes = response.bytes_stream();
            let mut buffer = String::new();

            use futures::StreamExt;
            while let Some(chunk) = bytes.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        yield Err(LlmError::Http(e));
                        break;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // Process complete lines
                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim().to_string();
                    buffer = buffer[pos + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    // SSE format: "data: {...}"
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            break;
                        }

                        match serde_json::from_str::<ChatStreamChunk>(data) {
                            Ok(chunk) => yield Ok(chunk),
                            Err(e) => yield Err(LlmError::Json(e)),
                        }
                    }
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
}
