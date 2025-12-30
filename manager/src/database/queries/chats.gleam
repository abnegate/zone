import database/connection.{type Connection}
import gleam/option.{type Option}
import models/chat.{
  type Chat, type ChatWithMessages, type CreateChatRequest, type Message,
  type SendMessageRequest,
}

/// List all chats, optionally filtered by archived status
/// (Placeholder - returns empty list until DB implementation)
pub fn list_chats(
  _db: Connection,
  _archived_filter: Option(Bool),
) -> Result(List(Chat), String) {
  // TODO: Implement with actual DB query
  Ok([])
}

/// Get a single chat by ID (without messages)
/// (Placeholder - returns None until DB implementation)
pub fn get_chat(_db: Connection, _id: String) -> Result(Option(Chat), String) {
  // TODO: Implement with actual DB query
  Ok(option.None)
}

/// Get a chat with all its messages
/// (Placeholder - returns None until DB implementation)
pub fn get_chat_with_messages(
  _db: Connection,
  _id: String,
) -> Result(Option(ChatWithMessages), String) {
  // TODO: Implement with actual DB query
  Ok(option.None)
}

/// Create a new chat
/// (Placeholder - returns error until DB implementation)
pub fn create_chat(
  _db: Connection,
  _req: CreateChatRequest,
) -> Result(Chat, String) {
  // TODO: Implement with actual DB query
  // Should also create initial message if first_message is Some
  Error("Database not yet implemented")
}

/// Update chat title
/// (Placeholder - returns None until DB implementation)
pub fn update_chat_title(
  _db: Connection,
  _id: String,
  _title: String,
) -> Result(Option(Chat), String) {
  // TODO: Implement with actual DB query
  Ok(option.None)
}

/// Archive a chat
/// (Placeholder - returns None until DB implementation)
pub fn archive_chat(
  _db: Connection,
  _id: String,
) -> Result(Option(Chat), String) {
  // TODO: Implement with actual DB query
  Ok(option.None)
}

/// Unarchive a chat
/// (Placeholder - returns None until DB implementation)
pub fn unarchive_chat(
  _db: Connection,
  _id: String,
) -> Result(Option(Chat), String) {
  // TODO: Implement with actual DB query
  Ok(option.None)
}

/// Delete a chat and all its messages
/// (Placeholder - returns false until DB implementation)
pub fn delete_chat(_db: Connection, _id: String) -> Result(Bool, String) {
  // TODO: Implement with actual DB query
  Ok(False)
}

// =============================================================================
// Message operations
// =============================================================================

/// List messages for a chat
/// (Placeholder - returns empty list until DB implementation)
pub fn list_messages(
  _db: Connection,
  _chat_id: String,
) -> Result(List(Message), String) {
  // TODO: Implement with actual DB query
  Ok([])
}

/// Get a single message by ID
/// (Placeholder - returns None until DB implementation)
pub fn get_message(
  _db: Connection,
  _id: String,
) -> Result(Option(Message), String) {
  // TODO: Implement with actual DB query
  Ok(option.None)
}

/// Add a user message to a chat
/// (Placeholder - returns error until DB implementation)
pub fn add_user_message(
  _db: Connection,
  _chat_id: String,
  _req: SendMessageRequest,
) -> Result(Message, String) {
  // TODO: Implement with actual DB query
  Error("Database not yet implemented")
}

/// Add an assistant message to a chat
/// (Placeholder - returns error until DB implementation)
pub fn add_assistant_message(
  _db: Connection,
  _chat_id: String,
  _content: String,
) -> Result(Message, String) {
  // TODO: Implement with actual DB query
  Error("Database not yet implemented")
}

/// Add a system message to a chat
/// (Placeholder - returns error until DB implementation)
pub fn add_system_message(
  _db: Connection,
  _chat_id: String,
  _content: String,
) -> Result(Message, String) {
  // TODO: Implement with actual DB query
  Error("Database not yet implemented")
}

/// Delete a message by ID
/// (Placeholder - returns false until DB implementation)
pub fn delete_message(_db: Connection, _id: String) -> Result(Bool, String) {
  // TODO: Implement with actual DB query
  Ok(False)
}

/// Update chat's updated_at timestamp
/// (Internal helper - placeholder until DB implementation)
pub fn touch_chat(_db: Connection, _id: String) -> Result(Nil, String) {
  // TODO: Implement with actual DB query
  Ok(Nil)
}
