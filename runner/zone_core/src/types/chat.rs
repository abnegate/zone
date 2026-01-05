//! Chat types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A chat conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: Option<String>,
    pub model: String,
    pub is_archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A chat with its messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatWithMessages {
    #[serde(flatten)]
    pub chat: Chat,
    pub messages: Vec<Message>,
}

/// Request to create a chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChatRequest {
    pub title: Option<String>,
    pub model: Option<String>,
}

/// Request to update a chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChatRequest {
    pub title: Option<String>,
}

/// A message in a chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<serde_json::Value>,
    pub tool_call_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Message role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    #[default]
    User,
    Assistant,
    Tool,
}

/// Request to create a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    pub role: MessageRole,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_chat() -> Chat {
        Chat {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            title: Some("Test Chat".to_string()),
            model: "gpt-4".to_string(),
            is_archived: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn create_test_message(chat_id: Uuid) -> Message {
        Message {
            id: Uuid::new_v4(),
            chat_id,
            role: MessageRole::User,
            content: "Hello, world!".to_string(),
            tool_calls: None,
            tool_call_id: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_message_role_default() {
        assert_eq!(MessageRole::default(), MessageRole::User);
    }

    #[test]
    fn test_message_role_serialization() {
        assert_eq!(
            serde_json::to_string(&MessageRole::System).unwrap(),
            "\"system\""
        );
        assert_eq!(
            serde_json::to_string(&MessageRole::User).unwrap(),
            "\"user\""
        );
        assert_eq!(
            serde_json::to_string(&MessageRole::Assistant).unwrap(),
            "\"assistant\""
        );
        assert_eq!(
            serde_json::to_string(&MessageRole::Tool).unwrap(),
            "\"tool\""
        );
    }

    #[test]
    fn test_message_role_deserialization() {
        let system: MessageRole = serde_json::from_str("\"system\"").unwrap();
        assert_eq!(system, MessageRole::System);

        let assistant: MessageRole = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(assistant, MessageRole::Assistant);
    }

    #[test]
    fn test_chat_serialization() {
        let chat = create_test_chat();
        let json = serde_json::to_string(&chat).unwrap();

        assert!(json.contains("Test Chat"));
        assert!(json.contains("gpt-4"));

        let deserialized: Chat = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, chat.title);
        assert_eq!(deserialized.model, chat.model);
    }

    #[test]
    fn test_chat_without_title() {
        let mut chat = create_test_chat();
        chat.title = None;

        let json = serde_json::to_string(&chat).unwrap();
        let deserialized: Chat = serde_json::from_str(&json).unwrap();
        assert!(deserialized.title.is_none());
    }

    #[test]
    fn test_chat_archived() {
        let mut chat = create_test_chat();
        chat.is_archived = true;

        let json = serde_json::to_string(&chat).unwrap();
        let deserialized: Chat = serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_archived);
    }

    #[test]
    fn test_chat_with_messages() {
        let chat = create_test_chat();
        let chat_id = chat.id;
        let messages = vec![create_test_message(chat_id), create_test_message(chat_id)];

        let chat_with_messages = ChatWithMessages { chat, messages };

        let json = serde_json::to_string(&chat_with_messages).unwrap();
        assert!(json.contains("Test Chat"));
        assert!(json.contains("Hello, world!"));

        let deserialized: ChatWithMessages = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.messages.len(), 2);
    }

    #[test]
    fn test_chat_with_no_messages() {
        let chat = create_test_chat();
        let chat_with_messages = ChatWithMessages {
            chat,
            messages: vec![],
        };

        let json = serde_json::to_string(&chat_with_messages).unwrap();
        let deserialized: ChatWithMessages = serde_json::from_str(&json).unwrap();
        assert!(deserialized.messages.is_empty());
    }

    #[test]
    fn test_message_serialization() {
        let message = create_test_message(Uuid::new_v4());
        let json = serde_json::to_string(&message).unwrap();

        assert!(json.contains("Hello, world!"));
        assert!(json.contains("user"));

        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.content, message.content);
    }

    #[test]
    fn test_message_with_tool_calls() {
        let mut message = create_test_message(Uuid::new_v4());
        message.role = MessageRole::Assistant;
        message.tool_calls = Some(json!([{
            "id": "call_123",
            "function": {
                "name": "read_file",
                "arguments": "{\"path\": \"/test.txt\"}"
            }
        }]));

        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("read_file"));
        assert!(json.contains("call_123"));
    }

    #[test]
    fn test_message_tool_result() {
        let mut message = create_test_message(Uuid::new_v4());
        message.role = MessageRole::Tool;
        message.tool_call_id = Some("call_123".to_string());
        message.content = "File contents: ...".to_string();

        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("tool"));
        assert!(json.contains("call_123"));
    }

    #[test]
    fn test_create_chat_request() {
        let request = CreateChatRequest {
            title: Some("New Chat".to_string()),
            model: Some("gpt-4o".to_string()),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("New Chat"));
        assert!(json.contains("gpt-4o"));
    }

    #[test]
    fn test_create_chat_request_minimal() {
        let request = CreateChatRequest {
            title: None,
            model: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: CreateChatRequest = serde_json::from_str(&json).unwrap();
        assert!(deserialized.title.is_none());
        assert!(deserialized.model.is_none());
    }

    #[test]
    fn test_update_chat_request() {
        let request = UpdateChatRequest {
            title: Some("Updated Title".to_string()),
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: UpdateChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, Some("Updated Title".to_string()));
    }

    #[test]
    fn test_create_message_request() {
        let request = CreateMessageRequest {
            role: MessageRole::User,
            content: "Hello!".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("user"));
        assert!(json.contains("Hello!"));
    }

    #[test]
    fn test_message_role_equality() {
        assert_eq!(MessageRole::User, MessageRole::User);
        assert_ne!(MessageRole::User, MessageRole::Assistant);
    }

    #[test]
    fn test_message_role_copy() {
        let role = MessageRole::System;
        let copied = role;
        assert_eq!(role, copied);
    }

    #[test]
    fn test_chat_clone() {
        let chat = create_test_chat();
        let cloned = chat.clone();
        assert_eq!(chat.id, cloned.id);
        assert_eq!(chat.model, cloned.model);
    }

    #[test]
    fn test_message_clone() {
        let message = create_test_message(Uuid::new_v4());
        let cloned = message.clone();
        assert_eq!(message.id, cloned.id);
        assert_eq!(message.content, cloned.content);
    }
}
