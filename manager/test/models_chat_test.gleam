import gleam/json
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import models/chat

// =============================================================================
// MessageRole conversion tests
// =============================================================================

pub fn role_to_string_user_test() {
  chat.role_to_string(chat.User)
  |> should.equal("user")
}

pub fn role_to_string_assistant_test() {
  chat.role_to_string(chat.Assistant)
  |> should.equal("assistant")
}

pub fn role_to_string_system_test() {
  chat.role_to_string(chat.System)
  |> should.equal("system")
}

pub fn role_from_string_user_test() {
  chat.role_from_string("user")
  |> should.be_ok()
  |> should.equal(chat.User)
}

pub fn role_from_string_assistant_test() {
  chat.role_from_string("assistant")
  |> should.be_ok()
  |> should.equal(chat.Assistant)
}

pub fn role_from_string_system_test() {
  chat.role_from_string("system")
  |> should.be_ok()
  |> should.equal(chat.System)
}

pub fn role_from_string_invalid_test() {
  chat.role_from_string("invalid")
  |> should.be_error()
}

pub fn role_from_string_empty_test() {
  chat.role_from_string("")
  |> should.be_error()
}

// =============================================================================
// CreateChatRequest decoding tests
// =============================================================================

pub fn decode_create_chat_request_minimal_test() {
  let json_str = "{\"model_name\": \"llama3.2\"}"

  chat.decode_create_request(json_str)
  |> should.be_ok()
  |> fn(req: chat.CreateChatRequest) {
    should.equal(req.model_name, "llama3.2")
    should.equal(req.first_message, None)
  }
}

pub fn decode_create_chat_request_with_message_test() {
  let json_str =
    "{\"model_name\": \"llama3.2\", \"first_message\": \"Hello, how are you?\"}"

  chat.decode_create_request(json_str)
  |> should.be_ok()
  |> fn(req: chat.CreateChatRequest) {
    should.equal(req.model_name, "llama3.2")
    should.equal(req.first_message, Some("Hello, how are you?"))
  }
}

pub fn decode_create_chat_request_missing_model_test() {
  let json_str = "{\"first_message\": \"Hello\"}"

  chat.decode_create_request(json_str)
  |> should.be_error()
}

pub fn decode_create_chat_request_invalid_json_test() {
  chat.decode_create_request("not json")
  |> should.be_error()
}

// =============================================================================
// SendMessageRequest decoding tests
// =============================================================================

pub fn decode_send_message_request_test() {
  let json_str = "{\"content\": \"What is the meaning of life?\"}"

  chat.decode_send_message_request(json_str)
  |> should.be_ok()
  |> fn(req: chat.SendMessageRequest) {
    should.equal(req.content, "What is the meaning of life?")
  }
}

pub fn decode_send_message_request_missing_content_test() {
  let json_str = "{}"

  chat.decode_send_message_request(json_str)
  |> should.be_error()
}

pub fn decode_send_message_request_empty_content_test() {
  let json_str = "{\"content\": \"\"}"

  chat.decode_send_message_request(json_str)
  |> should.be_ok()
  |> fn(req: chat.SendMessageRequest) { should.equal(req.content, "") }
}

// =============================================================================
// Chat to_json tests
// =============================================================================

pub fn chat_to_json_test() {
  let c =
    chat.Chat(
      id: "chat-id",
      title: "Test Conversation",
      model_name: "llama3.2",
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-01T00:00:00Z",
      archived: False,
    )

  let json_str =
    chat.to_json(c)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"id\":\"chat-id\""))
  should.be_true(string.contains(json_str, "\"title\":\"Test Conversation\""))
  should.be_true(string.contains(json_str, "\"model_name\":\"llama3.2\""))
  should.be_true(string.contains(json_str, "\"archived\":false"))
}

pub fn chat_to_json_archived_test() {
  let c =
    chat.Chat(
      id: "chat-id",
      title: "Archived Chat",
      model_name: "llama3.2",
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-01T00:00:00Z",
      archived: True,
    )

  let json_str =
    chat.to_json(c)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"archived\":true"))
}

pub fn message_to_json_test() {
  let msg =
    chat.Message(
      id: "msg-id",
      chat_id: "chat-id",
      role: chat.User,
      content: "Hello, world!",
      created_at: "2025-01-01T00:00:00Z",
    )

  let json_str =
    chat.message_to_json(msg)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"id\":\"msg-id\""))
  should.be_true(string.contains(json_str, "\"chat_id\":\"chat-id\""))
  should.be_true(string.contains(json_str, "\"role\":\"user\""))
  should.be_true(string.contains(json_str, "\"content\":\"Hello, world!\""))
}

pub fn chat_with_messages_to_json_test() {
  let c =
    chat.Chat(
      id: "chat-id",
      title: "Test Chat",
      model_name: "llama3.2",
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-01T00:00:00Z",
      archived: False,
    )

  let msg1 =
    chat.Message(
      id: "msg-1",
      chat_id: "chat-id",
      role: chat.User,
      content: "Hello",
      created_at: "2025-01-01T00:00:00Z",
    )

  let msg2 =
    chat.Message(
      id: "msg-2",
      chat_id: "chat-id",
      role: chat.Assistant,
      content: "Hi there!",
      created_at: "2025-01-01T00:00:01Z",
    )

  let cwm = chat.ChatWithMessages(chat: c, messages: [msg1, msg2])

  let json_str =
    chat.with_messages_to_json(cwm)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"id\":\"chat-id\""))
  should.be_true(string.contains(json_str, "\"messages\":"))
  should.be_true(string.contains(json_str, "\"role\":\"user\""))
  should.be_true(string.contains(json_str, "\"role\":\"assistant\""))
}
