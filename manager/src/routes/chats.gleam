import database/connection.{type Connection}
import database/queries/chats
import gleam/http
import gleam/json
import gleam/option.{None, Some}
import models/chat
import wisp.{type Request, type Response}

/// Route handler for /api/chats endpoints
pub fn handle_chats_route(
  req: Request,
  path: List(String),
  db: Connection,
) -> Response {
  case path {
    // GET /api/chats - List all chats
    // POST /api/chats - Create new chat
    [] -> handle_chats_collection(req, db)

    // GET /api/chats/:id - Get chat with messages
    // PATCH /api/chats/:id - Update chat
    // DELETE /api/chats/:id - Delete chat
    [chat_id] -> handle_chat_resource(req, db, chat_id)

    // POST /api/chats/:id/archive - Archive chat
    [chat_id, "archive"] -> handle_archive(req, db, chat_id)

    // POST /api/chats/:id/unarchive - Unarchive chat
    [chat_id, "unarchive"] -> handle_unarchive(req, db, chat_id)

    // GET /api/chats/:id/messages - List messages
    // POST /api/chats/:id/messages - Send message
    [chat_id, "messages"] -> handle_messages_collection(req, db, chat_id)

    // DELETE /api/chats/:id/messages/:message_id - Delete message
    [chat_id, "messages", message_id] ->
      handle_message_resource(req, db, chat_id, message_id)

    _ -> wisp.not_found()
  }
}

// =============================================================================
// Chats Collection Handlers
// =============================================================================

fn handle_chats_collection(req: Request, db: Connection) -> Response {
  case req.method {
    http.Get -> list_chats(req, db)
    http.Post -> create_chat(req, db)
    _ -> wisp.method_not_allowed([http.Get, http.Post])
  }
}

fn list_chats(req: Request, db: Connection) -> Response {
  let query = wisp.get_query(req)

  // Parse archived filter from query params
  let archived_filter = case get_query_param(query, "archived", "") {
    "true" -> Some(True)
    "false" -> Some(False)
    _ -> None
  }

  case chats.list_chats(db, archived_filter) {
    Ok(chat_list) -> {
      let json_response =
        json.object([
          #("success", json.bool(True)),
          #("chats", json.array(chat_list, chat.to_json)),
        ])
        |> json.to_string

      wisp.response(200)
      |> wisp.set_header("content-type", "application/json")
      |> wisp.string_body(json_response)
    }
    Error(err) -> json_error_response(500, err)
  }
}

fn create_chat(req: Request, db: Connection) -> Response {
  use body <- wisp.require_string_body(req)

  case chat.decode_create_request(body) {
    Ok(create_req) -> {
      case chats.create_chat(db, create_req) {
        Ok(new_chat) -> {
          let json_response =
            json.object([
              #("success", json.bool(True)),
              #("chat", chat.to_json(new_chat)),
            ])
            |> json.to_string

          wisp.response(201)
          |> wisp.set_header("content-type", "application/json")
          |> wisp.string_body(json_response)
        }
        Error(err) -> json_error_response(500, err)
      }
    }
    Error(_) -> json_error_response(400, "Invalid request body")
  }
}

// =============================================================================
// Single Chat Resource Handlers
// =============================================================================

fn handle_chat_resource(req: Request, db: Connection, chat_id: String) -> Response {
  case req.method {
    http.Get -> get_chat(db, chat_id)
    http.Patch -> update_chat(req, db, chat_id)
    http.Delete -> delete_chat(db, chat_id)
    _ -> wisp.method_not_allowed([http.Get, http.Patch, http.Delete])
  }
}

fn get_chat(db: Connection, chat_id: String) -> Response {
  case chats.get_chat_with_messages(db, chat_id) {
    Ok(Some(chat_with_messages)) -> {
      let json_response =
        json.object([
          #("success", json.bool(True)),
          #("chat", chat.with_messages_to_json(chat_with_messages)),
        ])
        |> json.to_string

      wisp.response(200)
      |> wisp.set_header("content-type", "application/json")
      |> wisp.string_body(json_response)
    }
    Ok(None) -> json_error_response(404, "Chat not found")
    Error(err) -> json_error_response(500, err)
  }
}

fn update_chat(req: Request, db: Connection, chat_id: String) -> Response {
  use body <- wisp.require_string_body(req)

  // Parse title from request body
  case parse_update_title(body) {
    Ok(title) -> {
      case chats.update_chat_title(db, chat_id, title) {
        Ok(Some(updated_chat)) -> {
          let json_response =
            json.object([
              #("success", json.bool(True)),
              #("chat", chat.to_json(updated_chat)),
            ])
            |> json.to_string

          wisp.response(200)
          |> wisp.set_header("content-type", "application/json")
          |> wisp.string_body(json_response)
        }
        Ok(None) -> json_error_response(404, "Chat not found")
        Error(err) -> json_error_response(500, err)
      }
    }
    Error(err) -> json_error_response(400, err)
  }
}

fn delete_chat(db: Connection, chat_id: String) -> Response {
  case chats.delete_chat(db, chat_id) {
    Ok(True) -> {
      let json_response =
        json.object([
          #("success", json.bool(True)),
          #("message", json.string("Chat deleted")),
        ])
        |> json.to_string

      wisp.response(200)
      |> wisp.set_header("content-type", "application/json")
      |> wisp.string_body(json_response)
    }
    Ok(False) -> json_error_response(404, "Chat not found")
    Error(err) -> json_error_response(500, err)
  }
}

// =============================================================================
// Archive/Unarchive Handlers
// =============================================================================

fn handle_archive(req: Request, db: Connection, chat_id: String) -> Response {
  case req.method {
    http.Post -> {
      case chats.archive_chat(db, chat_id) {
        Ok(Some(archived_chat)) -> {
          let json_response =
            json.object([
              #("success", json.bool(True)),
              #("chat", chat.to_json(archived_chat)),
            ])
            |> json.to_string

          wisp.response(200)
          |> wisp.set_header("content-type", "application/json")
          |> wisp.string_body(json_response)
        }
        Ok(None) -> json_error_response(404, "Chat not found")
        Error(err) -> json_error_response(500, err)
      }
    }
    _ -> wisp.method_not_allowed([http.Post])
  }
}

fn handle_unarchive(req: Request, db: Connection, chat_id: String) -> Response {
  case req.method {
    http.Post -> {
      case chats.unarchive_chat(db, chat_id) {
        Ok(Some(unarchived_chat)) -> {
          let json_response =
            json.object([
              #("success", json.bool(True)),
              #("chat", chat.to_json(unarchived_chat)),
            ])
            |> json.to_string

          wisp.response(200)
          |> wisp.set_header("content-type", "application/json")
          |> wisp.string_body(json_response)
        }
        Ok(None) -> json_error_response(404, "Chat not found")
        Error(err) -> json_error_response(500, err)
      }
    }
    _ -> wisp.method_not_allowed([http.Post])
  }
}

// =============================================================================
// Messages Handlers
// =============================================================================

fn handle_messages_collection(
  req: Request,
  db: Connection,
  chat_id: String,
) -> Response {
  case req.method {
    http.Get -> list_messages(db, chat_id)
    http.Post -> send_message(req, db, chat_id)
    _ -> wisp.method_not_allowed([http.Get, http.Post])
  }
}

fn list_messages(db: Connection, chat_id: String) -> Response {
  case chats.list_messages(db, chat_id) {
    Ok(messages) -> {
      let json_response =
        json.object([
          #("success", json.bool(True)),
          #("messages", json.array(messages, chat.message_to_json)),
        ])
        |> json.to_string

      wisp.response(200)
      |> wisp.set_header("content-type", "application/json")
      |> wisp.string_body(json_response)
    }
    Error(err) -> json_error_response(500, err)
  }
}

fn send_message(req: Request, db: Connection, chat_id: String) -> Response {
  use body <- wisp.require_string_body(req)

  case chat.decode_send_message_request(body) {
    Ok(send_req) -> {
      case chats.add_user_message(db, chat_id, send_req) {
        Ok(message) -> {
          let json_response =
            json.object([
              #("success", json.bool(True)),
              #("message", chat.message_to_json(message)),
            ])
            |> json.to_string

          wisp.response(201)
          |> wisp.set_header("content-type", "application/json")
          |> wisp.string_body(json_response)
        }
        Error(err) -> json_error_response(500, err)
      }
    }
    Error(_) -> json_error_response(400, "Invalid request body")
  }
}

fn handle_message_resource(
  req: Request,
  db: Connection,
  _chat_id: String,
  message_id: String,
) -> Response {
  case req.method {
    http.Delete -> delete_message(db, message_id)
    _ -> wisp.method_not_allowed([http.Delete])
  }
}

fn delete_message(db: Connection, message_id: String) -> Response {
  case chats.delete_message(db, message_id) {
    Ok(True) -> {
      let json_response =
        json.object([
          #("success", json.bool(True)),
          #("message", json.string("Message deleted")),
        ])
        |> json.to_string

      wisp.response(200)
      |> wisp.set_header("content-type", "application/json")
      |> wisp.string_body(json_response)
    }
    Ok(False) -> json_error_response(404, "Message not found")
    Error(err) -> json_error_response(500, err)
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

fn json_error_response(status: Int, message: String) -> Response {
  let error_json =
    json.object([
      #("success", json.bool(False)),
      #("error", json.string(message)),
    ])
    |> json.to_string

  wisp.response(status)
  |> wisp.set_header("content-type", "application/json")
  |> wisp.string_body(error_json)
}
