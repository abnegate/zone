import birl
import database/connection.{type Connection, query_error_to_string}
import gleam/dynamic/decode
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import models/chat.{
  type Chat, type ChatWithMessages, type CreateChatRequest, type Message,
  type MessageRole, type SendMessageRequest, Assistant, Chat, ChatWithMessages,
  Message, SendMessageRequest, System, User,
}
import pog

// =============================================================================
// Chat Queries
// =============================================================================

/// List all chats, optionally filtered by archived status
pub fn list_chats(
  db: Connection,
  archived_filter: Option(Bool),
) -> Result(List(Chat), String) {
  let sql = case archived_filter {
    None ->
      "SELECT id, title, model_name, created_at, updated_at, archived
       FROM chats ORDER BY updated_at DESC"
    Some(True) ->
      "SELECT id, title, model_name, created_at, updated_at, archived
       FROM chats WHERE archived = true ORDER BY updated_at DESC"
    Some(False) ->
      "SELECT id, title, model_name, created_at, updated_at, archived
       FROM chats WHERE archived = false ORDER BY updated_at DESC"
  }

  pog.query(sql)
  |> pog.returning(chat_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.rows })
  |> result.map_error(query_error_to_string)
}

/// Get a single chat by ID
pub fn get_chat(db: Connection, id: String) -> Result(Option(Chat), String) {
  let sql =
    "SELECT id, title, model_name, created_at, updated_at, archived
     FROM chats WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.returning(chat_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Get a chat with all its messages
pub fn get_chat_with_messages(
  db: Connection,
  id: String,
) -> Result(Option(ChatWithMessages), String) {
  case get_chat(db, id) {
    Ok(Some(chat)) -> {
      case list_messages(db, id) {
        Ok(messages) ->
          Ok(Some(ChatWithMessages(chat: chat, messages: messages)))
        Error(err) -> Error(err)
      }
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// Create a new chat
pub fn create_chat(
  db: Connection,
  req: CreateChatRequest,
) -> Result(Chat, String) {
  let now = birl.to_iso8601(birl.now())
  let title = case req.first_message {
    Some(msg) -> string.slice(msg, 0, 50)
    None -> "New Chat"
  }

  let sql =
    "INSERT INTO chats (title, model_name, created_at, updated_at, archived)
     VALUES ($1, $2, $3, $4, false)
     RETURNING id, title, model_name, created_at, updated_at, archived"

  let result =
    pog.query(sql)
    |> pog.parameter(pog.text(title))
    |> pog.parameter(pog.text(req.model_name))
    |> pog.parameter(pog.text(now))
    |> pog.parameter(pog.text(now))
    |> pog.returning(chat_row_decoder())
    |> pog.execute(db)

  case result {
    Ok(returned) -> {
      case list.first(returned.rows) {
        Ok(chat) -> {
          // If there's a first message, add it
          case req.first_message {
            Some(content) -> {
              let _ =
                add_user_message(
                  db,
                  chat.id,
                  SendMessageRequest(content: content),
                )
              Ok(chat)
            }
            None -> Ok(chat)
          }
        }
        Error(_) -> Error("Failed to create chat")
      }
    }
    Error(err) -> Error(query_error_to_string(err))
  }
}

/// Update chat title
pub fn update_chat_title(
  db: Connection,
  id: String,
  title: String,
) -> Result(Option(Chat), String) {
  let now = birl.to_iso8601(birl.now())
  let sql =
    "UPDATE chats SET title = $1, updated_at = $2 WHERE id = $3
     RETURNING id, title, model_name, created_at, updated_at, archived"

  pog.query(sql)
  |> pog.parameter(pog.text(title))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(id))
  |> pog.returning(chat_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Archive a chat
pub fn archive_chat(db: Connection, id: String) -> Result(Option(Chat), String) {
  let now = birl.to_iso8601(birl.now())
  let sql =
    "UPDATE chats SET archived = true, updated_at = $1 WHERE id = $2
     RETURNING id, title, model_name, created_at, updated_at, archived"

  pog.query(sql)
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(id))
  |> pog.returning(chat_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Unarchive a chat
pub fn unarchive_chat(
  db: Connection,
  id: String,
) -> Result(Option(Chat), String) {
  let now = birl.to_iso8601(birl.now())
  let sql =
    "UPDATE chats SET archived = false, updated_at = $1 WHERE id = $2
     RETURNING id, title, model_name, created_at, updated_at, archived"

  pog.query(sql)
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(id))
  |> pog.returning(chat_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Delete a chat and all its messages
pub fn delete_chat(db: Connection, id: String) -> Result(Bool, String) {
  // Messages are deleted via ON DELETE CASCADE
  let sql = "DELETE FROM chats WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.count > 0 })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Message Queries
// =============================================================================

/// List messages for a chat
pub fn list_messages(
  db: Connection,
  chat_id: String,
) -> Result(List(Message), String) {
  let sql =
    "SELECT id, chat_id, role, content, created_at
     FROM messages WHERE chat_id = $1 ORDER BY created_at ASC"

  pog.query(sql)
  |> pog.parameter(pog.text(chat_id))
  |> pog.returning(message_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.rows })
  |> result.map_error(query_error_to_string)
}

/// Get a single message by ID
pub fn get_message(
  db: Connection,
  id: String,
) -> Result(Option(Message), String) {
  let sql =
    "SELECT id, chat_id, role, content, created_at
     FROM messages WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.returning(message_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Add a user message to a chat
pub fn add_user_message(
  db: Connection,
  chat_id: String,
  req: SendMessageRequest,
) -> Result(Message, String) {
  add_message(db, chat_id, User, req.content)
}

/// Add an assistant message to a chat
pub fn add_assistant_message(
  db: Connection,
  chat_id: String,
  content: String,
) -> Result(Message, String) {
  add_message(db, chat_id, Assistant, content)
}

/// Add a system message to a chat
pub fn add_system_message(
  db: Connection,
  chat_id: String,
  content: String,
) -> Result(Message, String) {
  add_message(db, chat_id, System, content)
}

/// Internal: Add a message with specified role
fn add_message(
  db: Connection,
  chat_id: String,
  role: MessageRole,
  content: String,
) -> Result(Message, String) {
  let now = birl.to_iso8601(birl.now())
  let role_str = chat.role_to_string(role)

  let sql =
    "INSERT INTO messages (chat_id, role, content, created_at)
     VALUES ($1, $2, $3, $4)
     RETURNING id, chat_id, role, content, created_at"

  let result =
    pog.query(sql)
    |> pog.parameter(pog.text(chat_id))
    |> pog.parameter(pog.text(role_str))
    |> pog.parameter(pog.text(content))
    |> pog.parameter(pog.text(now))
    |> pog.returning(message_row_decoder())
    |> pog.execute(db)

  case result {
    Ok(returned) -> {
      case list.first(returned.rows) {
        Ok(message) -> {
          // Update chat's updated_at
          let _ = touch_chat(db, chat_id)
          Ok(message)
        }
        Error(_) -> Error("Failed to create message")
      }
    }
    Error(err) -> Error(query_error_to_string(err))
  }
}

/// Delete a message by ID
pub fn delete_message(db: Connection, id: String) -> Result(Bool, String) {
  let sql = "DELETE FROM messages WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Update chat's updated_at timestamp
pub fn touch_chat(db: Connection, id: String) -> Result(Nil, String) {
  let now = birl.to_iso8601(birl.now())
  let sql = "UPDATE chats SET updated_at = $1 WHERE id = $2"

  pog.query(sql)
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(id))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Decoders
// =============================================================================

fn chat_row_decoder() -> decode.Decoder(Chat) {
  use id <- decode.field(0, decode.string)
  use title <- decode.field(1, decode.string)
  use model_name <- decode.field(2, decode.string)
  use created_at <- decode.field(3, decode.string)
  use updated_at <- decode.field(4, decode.string)
  use archived <- decode.field(5, decode.bool)
  decode.success(Chat(
    id: id,
    title: title,
    model_name: model_name,
    created_at: created_at,
    updated_at: updated_at,
    archived: archived,
  ))
}

fn message_row_decoder() -> decode.Decoder(Message) {
  use id <- decode.field(0, decode.string)
  use chat_id <- decode.field(1, decode.string)
  use role_str <- decode.field(2, decode.string)
  use content <- decode.field(3, decode.string)
  use created_at <- decode.field(4, decode.string)
  let role = case chat.role_from_string(role_str) {
    Ok(r) -> r
    Error(_) -> User
  }
  decode.success(Message(
    id: id,
    chat_id: chat_id,
    role: role,
    content: content,
    created_at: created_at,
  ))
}
