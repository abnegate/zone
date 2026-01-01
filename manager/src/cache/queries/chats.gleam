/// Cached chats queries - wraps database queries with Valkey caching
///
/// Uses cache-aside pattern:
/// - Read: Check cache first, fallback to DB, cache result
/// - Write: Perform DB operation, then invalidate cache
import cache/connection as cache
import config
import database/connection.{type Connection}
import database/queries/chats as db_chats
import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}
import models/chat.{
  type Chat, type ChatWithMessages, type CreateChatRequest, type Message,
  type SendMessageRequest, Chat, ChatWithMessages, Message,
}

const entity_type = "chat"

const messages_entity_type = "messages"

// =============================================================================
// Cached Chat Queries
// =============================================================================

/// List all chats with caching
pub fn list_chats(
  db: Connection,
  cache_client: cache.CacheConnection,
  archived_filter: Option(Bool),
) -> Result(List(Chat), String) {
  let cache_key = case archived_filter {
    None -> cache.list_key(entity_type)
    Some(True) -> cache.filtered_list_key(entity_type, "archived:true")
    Some(False) -> cache.filtered_list_key(entity_type, "archived:false")
  }
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.list(chat_decoder())) {
        Ok(chats) -> Ok(chats)
        Error(_) ->
          fetch_and_cache_chats(
            db,
            cache_client,
            cache_key,
            ttl,
            archived_filter,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_chats(db, cache_client, cache_key, ttl, archived_filter)
    Error(_) -> db_chats.list_chats(db, archived_filter)
  }
}

fn fetch_and_cache_chats(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  archived_filter: Option(Bool),
) -> Result(List(Chat), String) {
  case db_chats.list_chats(db, archived_filter) {
    Ok(chats) -> {
      let json_str = json.to_string(json.array(chats, chat_to_json))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(chats)
    }
    Error(err) -> Error(err)
  }
}

/// Get a single chat by ID with caching
pub fn get_chat(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Option(Chat), String) {
  let cache_key = cache.entity_key(entity_type, id)
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, chat_decoder()) {
        Ok(chat) -> Ok(Some(chat))
        Error(_) -> fetch_and_cache_chat(db, cache_client, cache_key, ttl, id)
      }
    }
    Ok(None) -> fetch_and_cache_chat(db, cache_client, cache_key, ttl, id)
    Error(_) -> db_chats.get_chat(db, id)
  }
}

fn fetch_and_cache_chat(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  id: String,
) -> Result(Option(Chat), String) {
  case db_chats.get_chat(db, id) {
    Ok(Some(chat)) -> {
      let json_str = json.to_string(chat_to_json(chat))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(chat))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// Get a chat with all its messages (cached)
pub fn get_chat_with_messages(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Option(ChatWithMessages), String) {
  let cache_key = cache.entity_key(entity_type, id <> ":with_messages")
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, chat_with_messages_decoder()) {
        Ok(cwm) -> Ok(Some(cwm))
        Error(_) ->
          fetch_and_cache_chat_with_messages(
            db,
            cache_client,
            cache_key,
            ttl,
            id,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_chat_with_messages(db, cache_client, cache_key, ttl, id)
    Error(_) -> db_chats.get_chat_with_messages(db, id)
  }
}

fn fetch_and_cache_chat_with_messages(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  id: String,
) -> Result(Option(ChatWithMessages), String) {
  case db_chats.get_chat_with_messages(db, id) {
    Ok(Some(cwm)) -> {
      let json_str = json.to_string(chat_with_messages_to_json(cwm))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(cwm))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// List messages for a chat (cached)
pub fn list_messages(
  db: Connection,
  cache_client: cache.CacheConnection,
  chat_id: String,
) -> Result(List(Message), String) {
  let cache_key = cache.entity_key(messages_entity_type, chat_id)
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.list(message_decoder())) {
        Ok(messages) -> Ok(messages)
        Error(_) ->
          fetch_and_cache_messages(db, cache_client, cache_key, ttl, chat_id)
      }
    }
    Ok(None) ->
      fetch_and_cache_messages(db, cache_client, cache_key, ttl, chat_id)
    Error(_) -> db_chats.list_messages(db, chat_id)
  }
}

fn fetch_and_cache_messages(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  chat_id: String,
) -> Result(List(Message), String) {
  case db_chats.list_messages(db, chat_id) {
    Ok(messages) -> {
      let json_str = json.to_string(json.array(messages, message_to_json))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(messages)
    }
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Write Operations (with cache invalidation)
// =============================================================================

/// Create a new chat
pub fn create_chat(
  db: Connection,
  cache_client: cache.CacheConnection,
  req: CreateChatRequest,
) -> Result(Chat, String) {
  case db_chats.create_chat(db, req) {
    Ok(chat) -> {
      invalidate_chat_cache(cache_client)
      Ok(chat)
    }
    Error(err) -> Error(err)
  }
}

/// Update chat title
pub fn update_chat_title(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
  title: String,
) -> Result(Option(Chat), String) {
  case db_chats.update_chat_title(db, id, title) {
    Ok(result) -> {
      invalidate_chat_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Archive a chat
pub fn archive_chat(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Option(Chat), String) {
  case db_chats.archive_chat(db, id) {
    Ok(result) -> {
      invalidate_chat_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Unarchive a chat
pub fn unarchive_chat(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Option(Chat), String) {
  case db_chats.unarchive_chat(db, id) {
    Ok(result) -> {
      invalidate_chat_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Delete a chat
pub fn delete_chat(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Bool, String) {
  case db_chats.delete_chat(db, id) {
    Ok(result) -> {
      invalidate_chat_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Add a user message
pub fn add_user_message(
  db: Connection,
  cache_client: cache.CacheConnection,
  chat_id: String,
  req: SendMessageRequest,
) -> Result(Message, String) {
  case db_chats.add_user_message(db, chat_id, req) {
    Ok(message) -> {
      invalidate_messages_cache(cache_client, chat_id)
      Ok(message)
    }
    Error(err) -> Error(err)
  }
}

/// Add an assistant message
pub fn add_assistant_message(
  db: Connection,
  cache_client: cache.CacheConnection,
  chat_id: String,
  content: String,
) -> Result(Message, String) {
  case db_chats.add_assistant_message(db, chat_id, content) {
    Ok(message) -> {
      invalidate_messages_cache(cache_client, chat_id)
      Ok(message)
    }
    Error(err) -> Error(err)
  }
}

/// Add a system message
pub fn add_system_message(
  db: Connection,
  cache_client: cache.CacheConnection,
  chat_id: String,
  content: String,
) -> Result(Message, String) {
  case db_chats.add_system_message(db, chat_id, content) {
    Ok(message) -> {
      invalidate_messages_cache(cache_client, chat_id)
      Ok(message)
    }
    Error(err) -> Error(err)
  }
}

/// Delete a message
pub fn delete_message(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
  chat_id: String,
) -> Result(Bool, String) {
  case db_chats.delete_message(db, id) {
    Ok(result) -> {
      invalidate_messages_cache(cache_client, chat_id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Passthrough Operations (no caching needed)
// =============================================================================

/// Get a single message by ID (not cached - rarely used)
pub fn get_message(
  db: Connection,
  id: String,
) -> Result(Option(Message), String) {
  db_chats.get_message(db, id)
}

/// Touch chat timestamp (not cached)
pub fn touch_chat(db: Connection, id: String) -> Result(Nil, String) {
  db_chats.touch_chat(db, id)
}

// =============================================================================
// Cache Invalidation
// =============================================================================

fn invalidate_chat_cache(cache_client: cache.CacheConnection) -> Nil {
  let _ =
    cache.delete_pattern(cache_client, cache.invalidation_pattern(entity_type))
  Nil
}

fn invalidate_chat_by_id(cache_client: cache.CacheConnection, id: String) -> Nil {
  // Delete specific chat cache
  let _ = cache.delete(cache_client, cache.entity_key(entity_type, id))
  let _ =
    cache.delete(
      cache_client,
      cache.entity_key(entity_type, id <> ":with_messages"),
    )
  // Also invalidate list caches
  invalidate_chat_cache(cache_client)
}

fn invalidate_messages_cache(
  cache_client: cache.CacheConnection,
  chat_id: String,
) -> Nil {
  let _ =
    cache.delete(cache_client, cache.entity_key(messages_entity_type, chat_id))
  let _ =
    cache.delete(
      cache_client,
      cache.entity_key(entity_type, chat_id <> ":with_messages"),
    )
  Nil
}

// =============================================================================
// JSON Serialization
// =============================================================================

fn chat_to_json(c: Chat) -> json.Json {
  json.object([
    #("id", json.string(c.id)),
    #("title", json.string(c.title)),
    #("model_name", json.string(c.model_name)),
    #("created_at", json.string(c.created_at)),
    #("updated_at", json.string(c.updated_at)),
    #("archived", json.bool(c.archived)),
  ])
}

fn chat_decoder() -> decode.Decoder(Chat) {
  use id <- decode.field("id", decode.string)
  use title <- decode.field("title", decode.string)
  use model_name <- decode.field("model_name", decode.string)
  use created_at <- decode.field("created_at", decode.string)
  use updated_at <- decode.field("updated_at", decode.string)
  use archived <- decode.field("archived", decode.bool)
  decode.success(Chat(
    id: id,
    title: title,
    model_name: model_name,
    created_at: created_at,
    updated_at: updated_at,
    archived: archived,
  ))
}

fn message_to_json(m: Message) -> json.Json {
  json.object([
    #("id", json.string(m.id)),
    #("chat_id", json.string(m.chat_id)),
    #("role", json.string(chat.role_to_string(m.role))),
    #("content", json.string(m.content)),
    #("created_at", json.string(m.created_at)),
  ])
}

fn message_decoder() -> decode.Decoder(Message) {
  use id <- decode.field("id", decode.string)
  use chat_id <- decode.field("chat_id", decode.string)
  use role_str <- decode.field("role", decode.string)
  use content <- decode.field("content", decode.string)
  use created_at <- decode.field("created_at", decode.string)
  let role = case chat.role_from_string(role_str) {
    Ok(r) -> r
    Error(_) -> chat.User
  }
  decode.success(Message(
    id: id,
    chat_id: chat_id,
    role: role,
    content: content,
    created_at: created_at,
  ))
}

fn chat_with_messages_to_json(cwm: ChatWithMessages) -> json.Json {
  json.object([
    #("chat", chat_to_json(cwm.chat)),
    #("messages", json.array(cwm.messages, message_to_json)),
  ])
}

fn chat_with_messages_decoder() -> decode.Decoder(ChatWithMessages) {
  use chat <- decode.field("chat", chat_decoder())
  use messages <- decode.field("messages", decode.list(message_decoder()))
  decode.success(ChatWithMessages(chat: chat, messages: messages))
}
