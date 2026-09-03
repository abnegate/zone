//! LLM request and response types
//!
//! OpenAI-compatible types for chat completions with tool use.

use serde::{Deserialize, Serialize};

/// One part of a multimodal message body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

/// An image reference. Data URLs are accepted by OpenAI-compatible providers,
/// which is how an uploaded image reaches a vision model without object storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

/// A chat message.
///
/// `content` stays a plain string for every caller. `images` is serialised by
/// widening the body into OpenAI content parts, the only shape vision models
/// accept; a message with no images serialises exactly as it did before.
#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Option<String>,
    pub name: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
    #[serde(default, skip_deserializing)]
    pub images: Vec<String>,
}

impl Serialize for Message {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("role", &self.role)?;

        if self.images.is_empty() {
            if let Some(content) = &self.content {
                map.serialize_entry("content", content)?;
            }
        } else {
            let mut parts = Vec::with_capacity(self.images.len() + 1);
            if let Some(content) = &self.content {
                if !content.is_empty() {
                    parts.push(ContentPart::Text {
                        text: content.clone(),
                    });
                }
            }
            for url in &self.images {
                parts.push(ContentPart::ImageUrl {
                    image_url: ImageUrl { url: url.clone() },
                });
            }
            map.serialize_entry("content", &parts)?;
        }

        if let Some(name) = &self.name {
            map.serialize_entry("name", name)?;
        }
        if let Some(tool_calls) = &self.tool_calls {
            map.serialize_entry("tool_calls", tool_calls)?;
        }
        if let Some(tool_call_id) = &self.tool_call_id {
            map.serialize_entry("tool_call_id", tool_call_id)?;
        }
        map.end()
    }
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }
    }

    pub fn assistant_with_tools(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: None,
            name: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            images: Vec::new(),
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            images: Vec::new(),
        }
    }
}

/// Message role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool call request from the model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// Function call details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Tool definition for the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

impl ToolDefinition {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// Function definition for the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Chat completion request
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// Tool choice specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    /// Let the model decide
    Auto(String),
    /// Don't use tools
    None(String),
    /// Force a specific tool
    Specific {
        r#type: String,
        function: SpecificFunction,
    },
}

impl ToolChoice {
    pub fn auto() -> Self {
        Self::Auto("auto".to_string())
    }

    pub fn none() -> Self {
        Self::None("none".to_string())
    }

    pub fn specific(name: impl Into<String>) -> Self {
        Self::Specific {
            r#type: "function".to_string(),
            function: SpecificFunction { name: name.into() },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificFunction {
    pub name: String,
}

/// Chat completion response
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

/// A completion choice
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

/// Token usage statistics
#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Streaming chunk.
///
/// Only `choices` carries content. The envelope fields are optional because
/// OpenAI-compatible providers do not all send them on every chunk -- LiteLLM
/// omits `id` on at least one -- and a chunk that fails to deserialise aborts
/// the whole stream, losing the reply.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatStreamChunk {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub choices: Vec<StreamChoice>,
}

/// Streaming choice delta
#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    #[serde(default)]
    pub index: u32,
    pub delta: StreamDelta,
    pub finish_reason: Option<String>,
}

/// Delta content in streaming
#[derive(Debug, Clone, Deserialize, Default)]
pub struct StreamDelta {
    pub role: Option<Role>,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<StreamToolCall>>,
}

/// Streaming tool call
#[derive(Debug, Clone, Deserialize)]
pub struct StreamToolCall {
    pub index: u32,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    pub function: Option<StreamFunctionCall>,
}

/// Streaming function call
#[derive(Debug, Clone, Deserialize)]
pub struct StreamFunctionCall {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_system() {
        let msg = Message::system("You are a helpful assistant");
        assert_eq!(msg.role, Role::System);
        assert_eq!(msg.content, Some("You are a helpful assistant".to_string()));
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn test_message_user() {
        let msg = Message::user("Hello!");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, Some("Hello!".to_string()));
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("Hi there!");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content, Some("Hi there!".to_string()));
    }

    #[test]
    fn test_message_assistant_with_tools() {
        let tool_call = ToolCall {
            id: "call_123".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: r#"{"path": "/tmp/test.txt"}"#.to_string(),
            },
        };
        let msg = Message::assistant_with_tools(vec![tool_call]);
        assert_eq!(msg.role, Role::Assistant);
        assert!(msg.content.is_none());
        assert!(msg.tool_calls.is_some());
        assert_eq!(msg.tool_calls.unwrap().len(), 1);
    }

    #[test]
    fn test_message_tool_result() {
        let msg = Message::tool_result("call_123", "file contents here");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.content, Some("file contents here".to_string()));
        assert_eq!(msg.tool_call_id, Some("call_123".to_string()));
    }

    #[test]
    fn test_tool_definition() {
        let def = ToolDefinition::function(
            "read_file",
            "Read a file from disk",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        );
        assert_eq!(def.tool_type, "function");
        assert_eq!(def.function.name, "read_file");
        assert_eq!(def.function.description, "Read a file from disk");
    }

    #[test]
    fn test_tool_choice() {
        let auto = ToolChoice::auto();
        let none = ToolChoice::none();
        let specific = ToolChoice::specific("read_file");

        // Verify serialization works
        let auto_json = serde_json::to_string(&auto).unwrap();
        assert!(auto_json.contains("auto"));

        let none_json = serde_json::to_string(&none).unwrap();
        assert!(none_json.contains("none"));

        let specific_json = serde_json::to_string(&specific).unwrap();
        assert!(specific_json.contains("read_file"));
    }

    #[test]
    fn test_role_serialization() {
        assert_eq!(serde_json::to_string(&Role::System).unwrap(), "\"system\"");
        assert_eq!(serde_json::to_string(&Role::User).unwrap(), "\"user\"");
        assert_eq!(
            serde_json::to_string(&Role::Assistant).unwrap(),
            "\"assistant\""
        );
        assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), "\"tool\"");
    }

    #[test]
    fn test_chat_request_serialization() {
        let request = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("Hello")],
            tools: None,
            tool_choice: None,
            temperature: Some(0.7),
            max_tokens: Some(1000),
            stream: Some(false),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("gpt-4"));
        assert!(json.contains("Hello"));
        assert!(json.contains("0.7"));
    }

    #[test]
    fn test_chat_response_deserialization() {
        let json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.model, "gpt-4");
        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content,
            Some("Hello!".to_string())
        );
        assert!(response.usage.is_some());
        assert_eq!(response.usage.unwrap().total_tokens, 30);
    }

    // ==================== Message Serialization Roundtrip Tests ====================

    #[test]
    fn test_message_serialization_roundtrip_system() {
        let msg = Message::system("You are a helpful assistant");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.role, Role::System);
        assert_eq!(
            deserialized.content,
            Some("You are a helpful assistant".to_string())
        );
        assert!(deserialized.tool_calls.is_none());
        assert!(deserialized.tool_call_id.is_none());
    }

    #[test]
    fn test_message_serialization_roundtrip_user() {
        let msg = Message::user("Hello, how are you?");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.role, Role::User);
        assert_eq!(
            deserialized.content,
            Some("Hello, how are you?".to_string())
        );
    }

    #[test]
    fn test_message_serialization_roundtrip_assistant() {
        let msg = Message::assistant("I'm doing well, thank you!");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.role, Role::Assistant);
        assert_eq!(
            deserialized.content,
            Some("I'm doing well, thank you!".to_string())
        );
    }

    #[test]
    fn test_message_serialization_roundtrip_tool_result() {
        let msg = Message::tool_result("call_abc123", "File contents: Hello World");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.role, Role::Tool);
        assert_eq!(
            deserialized.content,
            Some("File contents: Hello World".to_string())
        );
        assert_eq!(deserialized.tool_call_id, Some("call_abc123".to_string()));
    }

    #[test]
    fn test_message_serialization_roundtrip_with_tool_calls() {
        let tool_call = ToolCall {
            id: "call_xyz789".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "write_file".to_string(),
                arguments: r#"{"path": "/tmp/file.txt", "content": "Hello"}"#.to_string(),
            },
        };
        let msg = Message::assistant_with_tools(vec![tool_call]);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.role, Role::Assistant);
        assert!(deserialized.content.is_none());
        assert!(deserialized.tool_calls.is_some());
        let calls = deserialized.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_xyz789");
        assert_eq!(calls[0].function.name, "write_file");
    }

    // ==================== ToolDefinition Tests ====================

    #[test]
    fn test_tool_definition_serialization_roundtrip() {
        let def = ToolDefinition::function(
            "search_files",
            "Search for files matching a pattern",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Glob pattern"},
                    "directory": {"type": "string", "description": "Directory to search"}
                },
                "required": ["pattern"]
            }),
        );

        let json = serde_json::to_string(&def).unwrap();
        let deserialized: ToolDefinition = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.tool_type, "function");
        assert_eq!(deserialized.function.name, "search_files");
        assert_eq!(
            deserialized.function.description,
            "Search for files matching a pattern"
        );
        assert!(deserialized.function.parameters.get("properties").is_some());
    }

    #[test]
    fn test_tool_definition_empty_parameters() {
        let def =
            ToolDefinition::function("get_time", "Get the current time", serde_json::json!({}));

        let json = serde_json::to_string(&def).unwrap();
        let deserialized: ToolDefinition = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.function.name, "get_time");
        assert!(
            deserialized
                .function
                .parameters
                .as_object()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_tool_definition_complex_parameters() {
        let def = ToolDefinition::function(
            "complex_tool",
            "A tool with complex parameters",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "nested": {
                        "type": "object",
                        "properties": {
                            "inner": {"type": "string"}
                        }
                    },
                    "array_param": {
                        "type": "array",
                        "items": {"type": "integer"}
                    }
                }
            }),
        );

        let json = serde_json::to_string(&def).unwrap();
        assert!(json.contains("nested"));
        assert!(json.contains("array_param"));

        let deserialized: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.function.name, "complex_tool");
    }

    // ==================== ToolCall Tests ====================

    #[test]
    fn test_tool_call_serialization_roundtrip() {
        let call = ToolCall {
            id: "call_test123".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "execute_command".to_string(),
                arguments: r#"{"command": "ls -la"}"#.to_string(),
            },
        };

        let json = serde_json::to_string(&call).unwrap();
        let deserialized: ToolCall = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, "call_test123");
        assert_eq!(deserialized.call_type, "function");
        assert_eq!(deserialized.function.name, "execute_command");
        assert_eq!(deserialized.function.arguments, r#"{"command": "ls -la"}"#);
    }

    #[test]
    fn test_tool_call_empty_arguments() {
        let call = ToolCall {
            id: "call_empty".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "no_args_function".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let json = serde_json::to_string(&call).unwrap();
        let deserialized: ToolCall = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.function.arguments, "{}");
    }

    // ==================== ChatRequest Tests ====================

    #[test]
    fn test_chat_request_minimal() {
        let request = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("Hello")],
            tools: None,
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("gpt-4"));
        assert!(json.contains("Hello"));
        // Optional fields should not appear when None
        assert!(!json.contains("temperature"));
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("stream"));
    }

    #[test]
    fn test_chat_request_with_tools() {
        let tool = ToolDefinition::function(
            "test_tool",
            "A test tool",
            serde_json::json!({"type": "object"}),
        );

        let request = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("Use the tool")],
            tools: Some(vec![tool]),
            tool_choice: Some(ToolChoice::auto()),
            temperature: Some(0.5),
            max_tokens: Some(2048),
            stream: Some(false),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("test_tool"));
        assert!(json.contains("A test tool"));
        assert!(json.contains("auto"));
    }

    #[test]
    fn test_chat_request_multiple_messages() {
        let request = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![
                Message::system("You are helpful"),
                Message::user("Hello"),
                Message::assistant("Hi there!"),
                Message::user("How are you?"),
            ],
            tools: None,
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("system"));
        assert!(json.contains("user"));
        assert!(json.contains("assistant"));
    }

    // ==================== ChatResponse Tests ====================

    #[test]
    fn test_chat_response_without_usage() {
        let json = r#"{
            "id": "chatcmpl-456",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Response"
                },
                "finish_reason": "stop"
            }]
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(response.usage.is_none());
    }

    #[test]
    fn test_chat_response_with_tool_calls() {
        let json = r#"{
            "id": "chatcmpl-789",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\": \"NYC\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(response.choices[0].message.content.is_none());
        assert!(response.choices[0].message.tool_calls.is_some());
        let tool_calls = response.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "get_weather");
    }

    #[test]
    fn test_chat_response_multiple_choices() {
        let json = r#"{
            "id": "chatcmpl-multi",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": "Response 1"},
                    "finish_reason": "stop"
                },
                {
                    "index": 1,
                    "message": {"role": "assistant", "content": "Response 2"},
                    "finish_reason": "stop"
                }
            ]
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.choices.len(), 2);
        assert_eq!(response.choices[0].index, 0);
        assert_eq!(response.choices[1].index, 1);
    }

    // ==================== ToolChoice Tests ====================

    #[test]
    fn test_tool_choice_auto_serialization() {
        let choice = ToolChoice::auto();
        let json = serde_json::to_string(&choice).unwrap();
        assert_eq!(json, "\"auto\"");
    }

    #[test]
    fn test_tool_choice_none_serialization() {
        let choice = ToolChoice::none();
        let json = serde_json::to_string(&choice).unwrap();
        assert_eq!(json, "\"none\"");
    }

    #[test]
    fn test_tool_choice_specific_serialization() {
        let choice = ToolChoice::specific("my_function");
        let json = serde_json::to_string(&choice).unwrap();
        assert!(json.contains("function"));
        assert!(json.contains("my_function"));
    }

    // ==================== Role Tests ====================

    #[test]
    fn test_role_deserialization() {
        let system: Role = serde_json::from_str("\"system\"").unwrap();
        let user: Role = serde_json::from_str("\"user\"").unwrap();
        let assistant: Role = serde_json::from_str("\"assistant\"").unwrap();
        let tool: Role = serde_json::from_str("\"tool\"").unwrap();

        assert_eq!(system, Role::System);
        assert_eq!(user, Role::User);
        assert_eq!(assistant, Role::Assistant);
        assert_eq!(tool, Role::Tool);
    }

    #[test]
    fn test_role_equality() {
        assert_eq!(Role::System, Role::System);
        assert_eq!(Role::User, Role::User);
        assert_ne!(Role::System, Role::User);
        assert_ne!(Role::Assistant, Role::Tool);
    }

    #[test]
    fn test_role_copy() {
        let role = Role::Assistant;
        let copied = role;
        assert_eq!(role, copied);
    }

    // ==================== Streaming Types Tests ====================

    #[test]
    fn test_chat_stream_chunk_deserialization() {
        let json = r#"{
            "id": "chatcmpl-stream",
            "object": "chat.completion.chunk",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {
                    "content": "Hello"
                },
                "finish_reason": null
            }]
        }"#;

        let chunk: ChatStreamChunk = serde_json::from_str(json).unwrap();
        assert_eq!(chunk.id.as_deref(), Some("chatcmpl-stream"));
        assert_eq!(chunk.choices[0].delta.content, Some("Hello".to_string()));
        assert!(chunk.choices[0].finish_reason.is_none());
    }

    #[test]
    fn test_chat_stream_chunk_with_role() {
        let json = r#"{
            "id": "chatcmpl-stream",
            "object": "chat.completion.chunk",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant"
                },
                "finish_reason": null
            }]
        }"#;

        let chunk: ChatStreamChunk = serde_json::from_str(json).unwrap();
        assert_eq!(chunk.choices[0].delta.role, Some(Role::Assistant));
    }

    #[test]
    fn test_chat_stream_chunk_with_tool_calls() {
        let json = r#"{
            "id": "chatcmpl-stream",
            "object": "chat.completion.chunk",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "test_func",
                            "arguments": "{\"arg\":"
                        }
                    }]
                },
                "finish_reason": null
            }]
        }"#;

        let chunk: ChatStreamChunk = serde_json::from_str(json).unwrap();
        let tool_calls = chunk.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, Some("call_123".to_string()));
        assert_eq!(
            tool_calls[0].function.as_ref().unwrap().name,
            Some("test_func".to_string())
        );
    }

    #[test]
    fn test_stream_delta_default() {
        let delta = StreamDelta::default();
        assert!(delta.role.is_none());
        assert!(delta.content.is_none());
        assert!(delta.tool_calls.is_none());
    }

    #[test]
    fn test_chat_stream_chunk_finish_reason() {
        let json = r#"{
            "id": "chatcmpl-stream",
            "object": "chat.completion.chunk",
            "created": 1677652288,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        }"#;

        let chunk: ChatStreamChunk = serde_json::from_str(json).unwrap();
        assert_eq!(chunk.choices[0].finish_reason, Some("stop".to_string()));
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_message_with_special_characters() {
        let msg = Message::user("Hello \"world\"! \n\t Special chars: <>&");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.content,
            Some("Hello \"world\"! \n\t Special chars: <>&".to_string())
        );
    }

    #[test]
    fn test_message_with_unicode() {
        let msg = Message::user("Hello, world! unicode: Rust is good.");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.content,
            Some("Hello, world! unicode: Rust is good.".to_string())
        );
    }

    #[test]
    fn test_message_with_empty_content() {
        let msg = Message::user("");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.content, Some("".to_string()));
    }

    #[test]
    fn test_tool_call_with_complex_json_arguments() {
        let complex_args =
            r#"{"nested": {"key": "value"}, "array": [1, 2, 3], "null_field": null}"#;
        let call = ToolCall {
            id: "call_complex".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "complex_function".to_string(),
                arguments: complex_args.to_string(),
            },
        };

        let json = serde_json::to_string(&call).unwrap();
        let deserialized: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.function.arguments, complex_args);
    }

    #[test]
    fn test_multiple_tool_calls_in_message() {
        let tool_calls = vec![
            ToolCall {
                id: "call_1".to_string(),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: "func_1".to_string(),
                    arguments: "{}".to_string(),
                },
            },
            ToolCall {
                id: "call_2".to_string(),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: "func_2".to_string(),
                    arguments: r#"{"param": "value"}"#.to_string(),
                },
            },
        ];

        let msg = Message::assistant_with_tools(tool_calls);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        let calls = deserialized.tool_calls.unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[1].id, "call_2");
    }

    #[test]
    fn test_usage_fields() {
        let json = r#"{
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150
        }"#;

        let usage: Usage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }
}
