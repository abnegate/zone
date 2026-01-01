import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import gleam/time/duration
import gleam/time/timestamp.{type Timestamp}
import models/chat.{
  type Chat, type ChatWithMessages, type CreateChatRequest, type Message,
  type MessageRole, Assistant, Chat, ChatWithMessages, Message,
  SendMessageRequest, System, User,
}
import youid/uuid

// =============================================================================
// Chat Queries (using Squirrel-generated SQL)
// =============================================================================

/// List all chats, optionally filtered by archived status
pub fn list_chats(
  db: Connection,
  archived_filter: Option(Bool),
) -> Result(List(Chat), String) {
  case archived_filter {
    None ->
      sql.list_chats_all(db)
      |> result.map(fn(returned) {
        list.map(returned.rows, row_to_chat_from_all)
      })
      |> result.map_error(query_error_to_string)
    Some(True) ->
      sql.list_chats_archived(db)
      |> result.map(fn(returned) {
        list.map(returned.rows, row_to_chat_from_archived)
      })
      |> result.map_error(query_error_to_string)
    Some(False) ->
      sql.list_chats_active(db)
      |> result.map(fn(returned) {
        list.map(returned.rows, row_to_chat_from_active)
      })
      |> result.map_error(query_error_to_string)
  }
}

/// Get a single chat by ID
pub fn get_chat(db: Connection, id: String) -> Result(Option(Chat), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      sql.get_chat_by_id(db, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_chat_from_get)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
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
  let now = timestamp.system_time()
  let title = case req.first_message {
    Some(msg) -> string.slice(msg, 0, 50)
    None -> "New Chat"
  }

  sql.create_chat(db, title, req.model_name, now, now)
  |> result.map_error(query_error_to_string)
  |> result.try(fn(returned) {
    case list.first(returned.rows) {
      Ok(row) -> {
        let chat = row_to_chat_from_create(row)
        // If there's a first message, add it
        case req.first_message {
          Some(content) -> {
            let _ =
              add_user_message(db, chat.id, SendMessageRequest(content: content))
            Ok(chat)
          }
          None -> Ok(chat)
        }
      }
      Error(_) -> Error("Failed to create chat")
    }
  })
}

/// Update chat title
pub fn update_chat_title(
  db: Connection,
  id: String,
  title: String,
) -> Result(Option(Chat), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      sql.update_chat_title(db, title, now, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_chat_from_update)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Archive a chat
pub fn archive_chat(db: Connection, id: String) -> Result(Option(Chat), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      sql.archive_chat(db, now, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_chat_from_archive)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Unarchive a chat
pub fn unarchive_chat(
  db: Connection,
  id: String,
) -> Result(Option(Chat), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      sql.unarchive_chat(db, now, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_chat_from_unarchive)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Delete a chat and all its messages
pub fn delete_chat(db: Connection, id: String) -> Result(Bool, String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      sql.delete_chat(db, uuid_id)
      |> result.map(fn(returned) { returned.count > 0 })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

// =============================================================================
// Message Queries
// =============================================================================

/// List messages for a chat
pub fn list_messages(
  db: Connection,
  chat_id: String,
) -> Result(List(Message), String) {
  case uuid.from_string(chat_id) {
    Ok(uuid_id) -> {
      sql.list_messages(db, uuid_id)
      |> result.map(fn(returned) {
        list.map(returned.rows, row_to_message_from_list)
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Get a single message by ID
pub fn get_message(
  db: Connection,
  id: String,
) -> Result(Option(Message), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      sql.get_message_by_id(db, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_message_from_get)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Add a user message to a chat
pub fn add_user_message(
  db: Connection,
  chat_id: String,
  req: chat.SendMessageRequest,
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
  case uuid.from_string(chat_id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      let role_str = chat.role_to_string(role)

      sql.create_message(db, uuid_id, role_str, content, now)
      |> result.map_error(query_error_to_string)
      |> result.try(fn(returned) {
        case list.first(returned.rows) {
          Ok(row) -> {
            // Update chat's updated_at
            let _ = touch_chat(db, chat_id)
            Ok(row_to_message_from_create(row))
          }
          Error(_) -> Error("Failed to create message")
        }
      })
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Delete a message by ID
pub fn delete_message(db: Connection, id: String) -> Result(Bool, String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      sql.delete_message(db, uuid_id)
      |> result.map(fn(returned) { returned.count > 0 })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Update chat's updated_at timestamp
pub fn touch_chat(db: Connection, id: String) -> Result(Nil, String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      sql.touch_chat(db, now, uuid_id)
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

// =============================================================================
// Row Mapping Helpers
// =============================================================================

fn timestamp_to_string(ts: Option(Timestamp)) -> String {
  case ts {
    Some(t) -> timestamp.to_rfc3339(t, duration.seconds(0))
    None -> ""
  }
}

fn bool_option_to_bool(opt: Option(Bool)) -> Bool {
  option.unwrap(opt, False)
}

fn row_to_chat_from_all(row: sql.ListChatsAllRow) -> Chat {
  Chat(
    id: uuid.to_string(row.id),
    title: row.title,
    model_name: row.model_name,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    archived: bool_option_to_bool(row.archived),
  )
}

fn row_to_chat_from_archived(row: sql.ListChatsArchivedRow) -> Chat {
  Chat(
    id: uuid.to_string(row.id),
    title: row.title,
    model_name: row.model_name,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    archived: bool_option_to_bool(row.archived),
  )
}

fn row_to_chat_from_active(row: sql.ListChatsActiveRow) -> Chat {
  Chat(
    id: uuid.to_string(row.id),
    title: row.title,
    model_name: row.model_name,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    archived: bool_option_to_bool(row.archived),
  )
}

fn row_to_chat_from_get(row: sql.GetChatByIdRow) -> Chat {
  Chat(
    id: uuid.to_string(row.id),
    title: row.title,
    model_name: row.model_name,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    archived: bool_option_to_bool(row.archived),
  )
}

fn row_to_chat_from_create(row: sql.CreateChatRow) -> Chat {
  Chat(
    id: uuid.to_string(row.id),
    title: row.title,
    model_name: row.model_name,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    archived: bool_option_to_bool(row.archived),
  )
}

fn row_to_chat_from_update(row: sql.UpdateChatTitleRow) -> Chat {
  Chat(
    id: uuid.to_string(row.id),
    title: row.title,
    model_name: row.model_name,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    archived: bool_option_to_bool(row.archived),
  )
}

fn row_to_chat_from_archive(row: sql.ArchiveChatRow) -> Chat {
  Chat(
    id: uuid.to_string(row.id),
    title: row.title,
    model_name: row.model_name,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    archived: bool_option_to_bool(row.archived),
  )
}

fn row_to_chat_from_unarchive(row: sql.UnarchiveChatRow) -> Chat {
  Chat(
    id: uuid.to_string(row.id),
    title: row.title,
    model_name: row.model_name,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    archived: bool_option_to_bool(row.archived),
  )
}

fn row_to_message_from_list(row: sql.ListMessagesRow) -> Message {
  let role = case chat.role_from_string(row.role) {
    Ok(r) -> r
    Error(_) -> User
  }
  Message(
    id: uuid.to_string(row.id),
    chat_id: uuid.to_string(row.chat_id),
    role: role,
    content: row.content,
    created_at: timestamp_to_string(row.created_at),
  )
}

fn row_to_message_from_get(row: sql.GetMessageByIdRow) -> Message {
  let role = case chat.role_from_string(row.role) {
    Ok(r) -> r
    Error(_) -> User
  }
  Message(
    id: uuid.to_string(row.id),
    chat_id: uuid.to_string(row.chat_id),
    role: role,
    content: row.content,
    created_at: timestamp_to_string(row.created_at),
  )
}

fn row_to_message_from_create(row: sql.CreateMessageRow) -> Message {
  let role = case chat.role_from_string(row.role) {
    Ok(r) -> r
    Error(_) -> User
  }
  Message(
    id: uuid.to_string(row.id),
    chat_id: uuid.to_string(row.chat_id),
    role: role,
    content: row.content,
    created_at: timestamp_to_string(row.created_at),
  )
}
