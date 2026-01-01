/// WebSocket handler for task execution progress streaming
import agents/executor
import agents/task_worker.{type ProgressMessage}
import agents/task_worker/types.{ExecutionCompleted, ExecutionFailed}
import auth/jwt as auth_jwt
import config
import database/connection
import database/queries/tasks
import gleam/bytes_tree
import gleam/dynamic/decode
import gleam/erlang/process.{type Subject}
import gleam/http/request.{type Request as HttpRequest}
import gleam/http/response
import gleam/json
import gleam/list
import gleam/option.{None, Some}
import gleam/result
import gleam/string
import gleam/uri
import mist.{
  type Connection, type ResponseData, type WebsocketConnection,
  type WebsocketMessage,
}
import websocket/pull

/// WebSocket state for task runs
pub type TaskWsState {
  TaskWsState(
    conn: WebsocketConnection,
    run_id: String,
    progress_subject: Subject(ProgressMessage),
    authenticated: Bool,
  )
}

/// Handle WebSocket upgrade with authentication for task runs
/// Path: /ws/tasks/:run_id - authentication happens via message after connection
pub fn handle_websocket_upgrade_with_auth(
  req: HttpRequest(Connection),
  run_id: String,
) -> response.Response(ResponseData) {
  handle_websocket_upgrade(req, run_id)
}

fn unauthorized_response(message: String) -> response.Response(ResponseData) {
  response.new(401)
  |> response.set_body(
    mist.Bytes(bytes_tree.from_string("Unauthorized: " <> message)),
  )
}

/// Handle WebSocket upgrade
fn handle_websocket_upgrade(
  req: HttpRequest(Connection),
  run_id: String,
) -> response.Response(ResponseData) {
  // Create a subject for receiving progress updates
  let progress_subject = process.new_subject()

  mist.websocket(
    request: req,
    on_init: fn(conn) {
      // Start listening for progress updates in the background
      let ws_conn = conn
      let subj = progress_subject
      let rid = run_id

      process.spawn(fn() { forward_progress_to_websocket(ws_conn, subj) })

      #(TaskWsState(conn, rid, progress_subject, False), None)
    },
    on_close: fn(_state) { Nil },
    handler: handle_websocket_message,
  )
}

/// Handle incoming WebSocket messages
fn handle_websocket_message(
  state: TaskWsState,
  message: WebsocketMessage(String),
  conn: WebsocketConnection,
) -> mist.Next(TaskWsState, String) {
  case message {
    mist.Text(text) -> {
      case state.authenticated {
        False -> {
          // First message must be authentication
          case parse_ws_auth_request(text) {
            Ok(token) -> {
              case verify_token(token) {
                True -> {
                  send_message(
                    conn,
                    json.object([#("type", json.string("authenticated"))])
                      |> json.to_string,
                  )
                  mist.continue(TaskWsState(
                    state.conn,
                    state.run_id,
                    state.progress_subject,
                    True,
                  ))
                }
                False -> {
                  send_error(conn, "Invalid authentication token")
                  mist.stop()
                }
              }
            }
            Error(err) -> {
              send_error(conn, err)
              mist.stop()
            }
          }
        }
        True -> {
          // Parse the incoming message
          case parse_ws_task_request(text) {
            Ok(request_type) -> {
              case request_type {
                StartRequest(task_id) -> {
                  // Start task execution
                  handle_start_request(state, task_id)
                }
                CancelRequest -> {
                  // Cancel running task
                  handle_cancel_request(state)
                }
                SubscribeRequest -> {
                  // Just subscribe to updates (already done)
                  send_message(
                    conn,
                    json.object([
                      #("type", json.string("subscribed")),
                      #("run_id", json.string(state.run_id)),
                    ])
                      |> json.to_string,
                  )
                  mist.continue(state)
                }
              }
            }
            Error(err) -> {
              send_error(conn, err)
              mist.continue(state)
            }
          }
        }
      }
    }
    mist.Binary(_) -> mist.continue(state)
    mist.Custom(_) -> mist.continue(state)
    mist.Closed | mist.Shutdown -> mist.stop()
  }
}

/// Request types for task WebSocket
type TaskRequest {
  StartRequest(task_id: String)
  CancelRequest
  SubscribeRequest
}

/// Parse WebSocket authentication request JSON
fn parse_ws_auth_request(text: String) -> Result(String, String) {
  let decoder = {
    use token <- decode.field("token", decode.string)
    decode.success(token)
  }

  case json.parse(text, decoder) {
    Ok(token) -> {
      case string.trim(token) {
        "" -> Error("Token cannot be empty")
        trimmed -> Ok(trimmed)
      }
    }
    Error(_) -> Error("Invalid auth request: expected {\"token\": \"...\"}")
  }
}

/// Verify JWT token
fn verify_token(token: String) -> Bool {
  let jwt_secret = config.get_jwt_secret()
  case auth_jwt.validate_token(token, jwt_secret) {
    Ok(_) -> True
    Error(_) -> False
  }
}

/// Parse WebSocket task request JSON
fn parse_ws_task_request(text: String) -> Result(TaskRequest, String) {
  let type_decoder = {
    use request_type <- decode.field("type", decode.string)
    decode.success(request_type)
  }

  case json.parse(text, type_decoder) {
    Ok("start") -> {
      let task_id_decoder = {
        use task_id <- decode.field("task_id", decode.string)
        decode.success(task_id)
      }
      case json.parse(text, task_id_decoder) {
        Ok(task_id) -> Ok(StartRequest(task_id))
        Error(_) -> Error("Missing task_id for start request")
      }
    }
    Ok("cancel") -> Ok(CancelRequest)
    Ok("subscribe") -> Ok(SubscribeRequest)
    Ok(other) -> Error("Unknown request type: " <> other)
    Error(_) -> Error("Invalid request format")
  }
}

/// Handle start task request
fn handle_start_request(
  state: TaskWsState,
  task_id: String,
) -> mist.Next(TaskWsState, String) {
  let db = connection.named_connection()

  case executor.start_task_execution(db, task_id, state.progress_subject) {
    Ok(run_id) -> {
      // Update state with new run_id
      let new_state =
        TaskWsState(state.conn, run_id, state.progress_subject, state.authenticated)

      send_message(
        state.conn,
        json.object([
          #("type", json.string("started")),
          #("run_id", json.string(run_id)),
          #("task_id", json.string(task_id)),
        ])
          |> json.to_string,
      )

      mist.continue(new_state)
    }
    Error(err) -> {
      send_error(state.conn, err)
      mist.continue(state)
    }
  }
}

/// Handle cancel task request
fn handle_cancel_request(state: TaskWsState) -> mist.Next(TaskWsState, String) {
  let db = connection.named_connection()

  case executor.cancel_task_execution(db, state.run_id) {
    Ok(_) -> {
      send_message(
        state.conn,
        json.object([
          #("type", json.string("cancelled")),
          #("run_id", json.string(state.run_id)),
        ])
          |> json.to_string,
      )
      mist.continue(state)
    }
    Error(err) -> {
      send_error(state.conn, err)
      mist.continue(state)
    }
  }
}

/// Forward progress updates to WebSocket
fn forward_progress_to_websocket(
  conn: WebsocketConnection,
  progress_subject: Subject(ProgressMessage),
) -> Nil {
  // Listen for progress messages indefinitely
  case process.receive(progress_subject, 3_600_000) {
    // 1 hour timeout
    Ok(msg) -> {
      let json_msg = task_worker.progress_to_json(msg)
      let _ = mist.send_text_frame(conn, json_msg)

      // Check if this is a completion message
      case msg {
        ExecutionCompleted(_, _, _) -> Nil
        ExecutionFailed(_, _) -> Nil
        _ -> forward_progress_to_websocket(conn, progress_subject)
      }
    }
    Error(_) -> {
      // Timeout - stop listening
      Nil
    }
  }
}

/// Send a message to the WebSocket
fn send_message(conn: WebsocketConnection, message: String) -> Nil {
  let _ = mist.send_text_frame(conn, message)
  Nil
}

/// Send an error message to the WebSocket
fn send_error(conn: WebsocketConnection, error: String) -> Nil {
  let msg =
    json.object([
      #("type", json.string("error")),
      #("error", json.string(error)),
    ])
    |> json.to_string

  let _ = mist.send_text_frame(conn, msg)
  Nil
}
