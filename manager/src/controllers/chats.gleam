import cache/queries/chats as cached_chats
import gleam/http
import gleam/json
import gleam/option.{None, Some}
import models/chat
import util/validation
import web.{type Context}
import wisp.{type Request, type Response}

/// Route handler for /api/chats endpoints
pub fn handle_chats_route(
  req: Request,
  path: List(String),
  ctx: Context,
) -> Response {
  case path {
    // GET /api/chats - List all chats
    // POST /api/chats - Create new chat
    [] -> handle_chats_collection(req, ctx)

    // GET /api/chats/:id - Get chat with messages
    // PATCH /api/chats/:id - Update chat
    // DELETE /api/chats/:id - Delete chat
    [chat_id] -> handle_chat_resource(req, ctx, chat_id)

    // POST /api/chats/:id/archive - Archive chat
    [chat_id, "archive"] -> handle_archive(req, ctx, chat_id)

    // POST /api/chats/:id/unarchive - Unarchive chat
    [chat_id, "unarchive"] -> handle_unarchive(req, ctx, chat_id)

    // GET /api/chats/:id/messages - List messages
    // POST /api/chats/:id/messages - Send message
    [chat_id, "messages"] -> handle_messages_collection(req, ctx, chat_id)

    // DELETE /api/chats/:id/messages/:message_id - Delete message
    [chat_id, "messages", message_id] ->
      handle_message_resource(req, ctx, chat_id, message_id)

    _ -> wisp.not_found()
  }
}

// =============================================================================
// Chats Collection Handlers
// =============================================================================

fn handle_chats_collection(req: Request, ctx: Context) -> Response {
  case req.method {
    http.Get -> list_chats(req, ctx)
    http.Post -> create_chat(req, ctx)
    _ -> wisp.method_not_allowed([http.Get, http.Post])
  }
}

fn list_chats(req: Request, ctx: Context) -> Response {
  let query = wisp.get_query(req)

  // Parse archived filter from query params
  let archived_filter = case get_query_param(query, "archived", "") {
    "true" -> Some(True)
    "false" -> Some(False)
    _ -> None
  }

  case cached_chats.list_chats(ctx.db, ctx.cache, archived_filter) {
    Ok(chat_list) ->
      web.json_success([#("chats", json.array(chat_list, chat.to_json))])
    Error(err) -> web.internal_error(err)
  }
}

fn create_chat(req: Request, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case chat.decode_create_request(body) {
    Ok(create_req) -> {
      case cached_chats.create_chat(ctx.db, ctx.cache, create_req) {
        Ok(new_chat) -> web.json_created([#("chat", chat.to_json(new_chat))])
        Error(err) -> web.internal_error(err)
      }
    }
    Error(_) -> web.bad_request("Invalid request body")
  }
}

// =============================================================================
// Single Chat Resource Handlers
// =============================================================================

fn handle_chat_resource(req: Request, ctx: Context, chat_id: String) -> Response {
  // Validate UUID format
  case validation.validate_uuid(chat_id, "chat_id") {
    Error(err) -> web.bad_request(err)
    Ok(_) -> {
      case req.method {
        http.Get -> get_chat(ctx, chat_id)
        http.Patch -> update_chat(req, ctx, chat_id)
        http.Delete -> delete_chat(ctx, chat_id)
        _ -> wisp.method_not_allowed([http.Get, http.Patch, http.Delete])
      }
    }
  }
}

fn get_chat(ctx: Context, chat_id: String) -> Response {
  case cached_chats.get_chat_with_messages(ctx.db, ctx.cache, chat_id) {
    Ok(Some(chat_with_messages)) ->
      web.json_success([
        #("chat", chat.with_messages_to_json(chat_with_messages)),
      ])
    Ok(None) -> web.not_found("Chat not found")
    Error(err) -> web.internal_error(err)
  }
}

fn update_chat(req: Request, ctx: Context, chat_id: String) -> Response {
  use body <- wisp.require_string_body(req)

  // Parse title from request body
  case parse_update_title(body) {
    Ok(title) -> {
      case cached_chats.update_chat_title(ctx.db, ctx.cache, chat_id, title) {
        Ok(Some(updated_chat)) ->
          web.json_success([#("chat", chat.to_json(updated_chat))])
        Ok(None) -> web.not_found("Chat not found")
        Error(err) -> web.internal_error(err)
      }
    }
    Error(err) -> web.bad_request(err)
  }
}

fn delete_chat(ctx: Context, chat_id: String) -> Response {
  case cached_chats.delete_chat(ctx.db, ctx.cache, chat_id) {
    Ok(True) -> web.json_success([#("message", json.string("Chat deleted"))])
    Ok(False) -> web.not_found("Chat not found")
    Error(err) -> web.internal_error(err)
  }
}

// =============================================================================
// Archive/Unarchive Handlers
// =============================================================================

fn handle_archive(req: Request, ctx: Context, chat_id: String) -> Response {
  case validation.validate_uuid(chat_id, "chat_id") {
    Error(err) -> web.bad_request(err)
    Ok(_) -> {
      case req.method {
        http.Post -> {
          case cached_chats.archive_chat(ctx.db, ctx.cache, chat_id) {
            Ok(Some(archived_chat)) ->
              web.json_success([#("chat", chat.to_json(archived_chat))])
            Ok(None) -> web.not_found("Chat not found")
            Error(err) -> web.internal_error(err)
          }
        }
        _ -> wisp.method_not_allowed([http.Post])
      }
    }
  }
}

fn handle_unarchive(req: Request, ctx: Context, chat_id: String) -> Response {
  case validation.validate_uuid(chat_id, "chat_id") {
    Error(err) -> web.bad_request(err)
    Ok(_) -> {
      case req.method {
        http.Post -> {
          case cached_chats.unarchive_chat(ctx.db, ctx.cache, chat_id) {
            Ok(Some(unarchived_chat)) ->
              web.json_success([#("chat", chat.to_json(unarchived_chat))])
            Ok(None) -> web.not_found("Chat not found")
            Error(err) -> web.internal_error(err)
          }
        }
        _ -> wisp.method_not_allowed([http.Post])
      }
    }
  }
}

// =============================================================================
// Messages Handlers
// =============================================================================

fn handle_messages_collection(
  req: Request,
  ctx: Context,
  chat_id: String,
) -> Response {
  case validation.validate_uuid(chat_id, "chat_id") {
    Error(err) -> web.bad_request(err)
    Ok(_) -> {
      case req.method {
        http.Get -> list_messages(ctx, chat_id)
        http.Post -> send_message(req, ctx, chat_id)
        _ -> wisp.method_not_allowed([http.Get, http.Post])
      }
    }
  }
}

fn list_messages(ctx: Context, chat_id: String) -> Response {
  case cached_chats.list_messages(ctx.db, ctx.cache, chat_id) {
    Ok(messages) ->
      web.json_success([
        #("messages", json.array(messages, chat.message_to_json)),
      ])
    Error(err) -> web.internal_error(err)
  }
}

fn send_message(req: Request, ctx: Context, chat_id: String) -> Response {
  use body <- wisp.require_string_body(req)

  case chat.decode_send_message_request(body) {
    Ok(send_req) -> {
      case cached_chats.add_user_message(ctx.db, ctx.cache, chat_id, send_req) {
        Ok(message) ->
          web.json_created([#("message", chat.message_to_json(message))])
        Error(err) -> web.internal_error(err)
      }
    }
    Error(_) -> web.bad_request("Invalid request body")
  }
}

fn handle_message_resource(
  req: Request,
  ctx: Context,
  chat_id: String,
  message_id: String,
) -> Response {
  case validation.validate_uuid(chat_id, "chat_id") {
    Error(err) -> web.bad_request(err)
    Ok(_) -> {
      case validation.validate_uuid(message_id, "message_id") {
        Error(err) -> web.bad_request(err)
        Ok(_) -> {
          case req.method {
            http.Delete -> delete_message(ctx, chat_id, message_id)
            _ -> wisp.method_not_allowed([http.Delete])
          }
        }
      }
    }
  }
}

fn delete_message(ctx: Context, chat_id: String, message_id: String) -> Response {
  case cached_chats.delete_message(ctx.db, ctx.cache, message_id, chat_id) {
    Ok(True) -> web.json_success([#("message", json.string("Message deleted"))])
    Ok(False) -> web.not_found("Message not found")
    Error(err) -> web.internal_error(err)
  }
}

// =============================================================================
// Utility Functions
// =============================================================================

fn get_query_param(
  query: List(#(String, String)),
  key: String,
  default: String,
) -> String {
  case query {
    [] -> default
    [#(k, v), ..rest] ->
      case k == key {
        True -> v
        False -> get_query_param(rest, key, default)
      }
  }
}

fn parse_update_title(body: String) -> Result(String, String) {
  let decoder = {
    use title <- decode.field("title", decode.string)
    decode.success(title)
  }

  case json.parse(body, decoder) {
    Ok(title) -> Ok(title)
    Error(_) -> Error("Invalid request: expected {\"title\": \"...\"}")
  }
}

import gleam/dynamic/decode
