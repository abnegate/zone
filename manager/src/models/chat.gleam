import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None}

/// Message role enum
pub type MessageRole {
  User
  Assistant
  System
}

/// Chat entity
pub type Chat {
  Chat(
    id: String,
    title: String,
    model_name: String,
    created_at: String,
    updated_at: String,
    archived: Bool,
  )
}

/// Message entity
pub type Message {
  Message(
    id: String,
    chat_id: String,
    role: MessageRole,
    content: String,
    created_at: String,
  )
}

/// Chat with messages
pub type ChatWithMessages {
  ChatWithMessages(chat: Chat, messages: List(Message))
}

/// Request to create a new chat
pub type CreateChatRequest {
  CreateChatRequest(model_name: String, first_message: Option(String))
}

/// Request to send a message
pub type SendMessageRequest {
  SendMessageRequest(content: String)
}

/// Convert MessageRole to string for database
pub fn role_to_string(role: MessageRole) -> String {
  case role {
    User -> "user"
    Assistant -> "assistant"
    System -> "system"
  }
}

/// Parse string to MessageRole
pub fn role_from_string(s: String) -> Result(MessageRole, Nil) {
  case s {
    "user" -> Ok(User)
    "assistant" -> Ok(Assistant)
    "system" -> Ok(System)
    _ -> Error(Nil)
  }
}

/// Decoder for MessageRole from database string
pub fn role_decoder() -> decode.Decoder(MessageRole) {
  decode.string
  |> decode.then(fn(s) {
    case role_from_string(s) {
      Ok(role) -> decode.success(role)
      Error(_) -> decode.failure(User, "MessageRole")
    }
  })
}

/// Decode CreateChatRequest from JSON
pub fn decode_create_request(
  data: String,
) -> Result(CreateChatRequest, json.DecodeError) {
  let decoder = {
    use model_name <- decode.field("model_name", decode.string)
    use first_message <- decode.optional_field(
      "first_message",
      None,
      decode.optional(decode.string),
    )
    decode.success(CreateChatRequest(
      model_name: model_name,
      first_message: first_message,
    ))
  }

  json.parse(data, decoder)
}

/// Decode SendMessageRequest from JSON
pub fn decode_send_message_request(
  data: String,
) -> Result(SendMessageRequest, json.DecodeError) {
  let decoder = {
    use content <- decode.field("content", decode.string)
    decode.success(SendMessageRequest(content: content))
  }

  json.parse(data, decoder)
}

/// Convert Chat to JSON
pub fn to_json(chat: Chat) -> json.Json {
  json.object([
    #("id", json.string(chat.id)),
    #("title", json.string(chat.title)),
    #("model_name", json.string(chat.model_name)),
    #("created_at", json.string(chat.created_at)),
    #("updated_at", json.string(chat.updated_at)),
    #("archived", json.bool(chat.archived)),
  ])
}

/// Convert Message to JSON
pub fn message_to_json(message: Message) -> json.Json {
  json.object([
    #("id", json.string(message.id)),
    #("chat_id", json.string(message.chat_id)),
    #("role", json.string(role_to_string(message.role))),
    #("content", json.string(message.content)),
    #("created_at", json.string(message.created_at)),
  ])
}

/// Convert ChatWithMessages to JSON
pub fn with_messages_to_json(cwm: ChatWithMessages) -> json.Json {
  json.object([
    #("id", json.string(cwm.chat.id)),
    #("title", json.string(cwm.chat.title)),
    #("model_name", json.string(cwm.chat.model_name)),
    #("created_at", json.string(cwm.chat.created_at)),
    #("updated_at", json.string(cwm.chat.updated_at)),
    #("archived", json.bool(cwm.chat.archived)),
    #("messages", json.array(cwm.messages, message_to_json)),
  ])
}
