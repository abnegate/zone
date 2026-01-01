import birl
import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import models/chat.{
  type Chat, type ChatWithMessages, type CreateChatRequest, type Message,
  type MessageRole, type SendMessageRequest, Assistant, Chat, ChatWithMessages,
  Message, SendMessageRequest, System, User,
}

// =============================================================================
// Chat Queries (using Squirrel-generated SQL)
// =============================================================================

/// List all chats, optionally filtered by archived status
pub fn list_chats(
  db: Connection,
  archived_filter: Option(Bool),
) -> Result(List(Chat), String) {
  let result = case archived_filter {
    None -> sql.list_chats_all(db)
    Some(True) -> sql.list_chats_archived(db)
    Some(False) -> sql.list_chats_active(db)
  }

  result
  |> result.map(fn(rows) { list.map(rows, row_to_chat) })
  |> result.map_error(query_error_to_string)
}

/// Get a single chat by ID
pub fn get_chat(db: Connection, id: String) -> Result(Option(Chat), String) {
  sql.get_chat_by_id(db, id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_chat)
    |> option.from_result
  })
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

  let result =
    sql.create_chat(db, title, req.model_name, now, now)
    |> result.map_error(query_error_to_string)

  case result {
    Ok(rows) -> {
      case list.first(rows) {
        Ok(row) -> {
          let chat = row_to_chat(row)
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
    Error(err) -> Error(err)
  }
}

/// Update chat title
pub fn update_chat_title(
  db: Connection,
  id: String,
  title: String,
) -> Result(Option(Chat), String) {
  let now = birl.to_iso8601(birl.now())
  sql.update_chat_title(db, title, now, id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_chat)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Archive a chat
pub fn archive_chat(db: Connection, id: String) -> Result(Option(Chat), String) {
  let now = birl.to_iso8601(birl.now())
  sql.archive_chat(db, now, id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_chat)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Unarchive a chat
pub fn unarchive_chat(
  db: Connection,
  id: String,
) -> Result(Option(Chat), String) {
  let now = birl.to_iso8601(birl.now())
  sql.unarchive_chat(db, now, id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_chat)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Delete a chat and all its messages
pub fn delete_chat(db: Connection, id: String) -> Result(Bool, String) {
  sql.delete_chat(db, id)
  |> result.map(fn(count) { count > 0 })
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
  sql.list_messages(db, chat_id)
  |> result.map(fn(rows) { list.map(rows, row_to_message) })
  |> result.map_error(query_error_to_string)
}

/// Get a single message by ID
pub fn get_message(
  db: Connection,
  id: String,
) -> Result(Option(Message), String) {
  sql.get_message_by_id(db, id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_message)
    |> option.from_result
  })
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

  let result =
    sql.create_message(db, chat_id, role_str, content, now)
    |> result.map_error(query_error_to_string)

  case result {
    Ok(rows) -> {
      case list.first(rows) {
        Ok(row) -> {
          // Update chat's updated_at
          let _ = touch_chat(db, chat_id)
          Ok(row_to_message(row))
        }
        Error(_) -> Error("Failed to create message")
      }
    }
    Error(err) -> Error(err)
  }
}

/// Delete a message by ID
pub fn delete_message(db: Connection, id: String) -> Result(Bool, String) {
  sql.delete_message(db, id)
  |> result.map(fn(count) { count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Update chat's updated_at timestamp
pub fn touch_chat(db: Connection, id: String) -> Result(Nil, String) {
  let now = birl.to_iso8601(birl.now())
  sql.touch_chat(db, now, id)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Mapping
// =============================================================================

fn row_to_chat(row: sql.ListChatsAllRow) -> Chat {
  Chat(
    id: row.id,
    title: row.title,
    model_name: row.model_name,
    created_at: row.created_at,
    updated_at: row.updated_at,
    archived: row.archived,
  )
}

fn row_to_message(row: sql.ListMessagesRow) -> Message {
  let role = case chat.role_from_string(row.role) {
    Ok(r) -> r
    Error(_) -> User
  }
  Message(
    id: row.id,
    chat_id: row.chat_id,
    role: role,
    content: row.content,
    created_at: row.created_at,
  )
}
