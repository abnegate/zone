import config
import controllers/models
import gleam/bytes_tree
import gleam/dynamic/decode
import gleam/erlang/process
import gleam/http/request.{type Request as HttpRequest}
import gleam/http/response
import gleam/json
import gleam/list
import gleam/option.{None, Some}
import gleam/result
import gleam/string
import gleam/uri
import httpp/jsonl.{Closed, Line}
import mist.{type Connection, type ResponseData, type WebsocketConnection, type WebsocketMessage}
import pull_stream

/// WebSocket state
pub type WsState {
  WsState(conn: WebsocketConnection)
}

/// Handle WebSocket upgrade with authentication
pub fn handle_websocket_upgrade_with_auth(
  req: HttpRequest(Connection),
) -> response.Response(ResponseData) {
  let expected_key = config.get_manager_api_key()

  case expected_key {
    "" -> {
      // No API key configured - reject
      response.new(401)
      |> response.set_body(mist.Bytes(bytes_tree.from_string("Unauthorized: API key not configured")))
    }
    key -> {
      // Parse query string to get api_key parameter
      let query_params = case req.query {
        Some(query) -> parse_query_params(query)
        None -> []
      }

      let provided_key = case list.find(query_params, fn(pair) { pair.0 == "api_key" }) {
        Ok(pair) -> pair.1
        Error(_) -> ""
      }

      case provided_key == key {
        True -> handle_websocket_upgrade(req)
        False -> {
          response.new(401)
          |> response.set_body(mist.Bytes(bytes_tree.from_string("Unauthorized: Invalid API key")))
        }
      }
    }
  }
}

/// Parse query string parameters into key-value pairs
pub fn parse_query_params(query: String) -> List(#(String, String)) {
  query
  |> string.split("&")
  |> list.filter(fn(p) { p != "" })
  |> list.filter_map(fn(param) {
    case string.split_once(param, "=") {
      Ok(#(key, value)) -> Ok(#(key, uri.percent_decode(value) |> result.unwrap(value)))
      Error(_) -> Ok(#(param, ""))
    }
  })
}

/// Handle WebSocket upgrade
fn handle_websocket_upgrade(
  req: HttpRequest(Connection),
) -> response.Response(ResponseData) {
  mist.websocket(
    request: req,
    on_init: fn(conn) { #(WsState(conn), None) },
    on_close: fn(_state) { Nil },
    handler: handle_websocket_message,
  )
}

/// Handle incoming WebSocket messages
fn handle_websocket_message(
  state: WsState,
  message: WebsocketMessage(String),
  conn: WebsocketConnection,
) -> mist.Next(WsState, String) {
  case message {
    mist.Text(text) -> {
      // Parse the incoming message to get the model name
      case parse_ws_pull_request(text) {
        Ok(model_name) -> {
          // Start the pull in the current process
          do_streaming_pull(conn, model_name)
          mist.continue(WsState(conn))
        }
        Error(err) -> {
          // Send error and continue
          let _ = mist.send_text_frame(conn, pull_stream.error_to_json(err))
          mist.continue(WsState(conn))
        }
      }
    }
    mist.Binary(_) -> mist.continue(state)
    mist.Custom(_) -> mist.continue(state)
    mist.Closed | mist.Shutdown -> mist.stop()
  }
}

/// Parse WebSocket pull request JSON
pub fn parse_ws_pull_request(text: String) -> Result(String, String) {
  let decoder = {
    use model <- decode.field("model", decode.string)
    decode.success(model)
  }

  case json.parse(text, decoder) {
    Ok(model_name) -> {
      case string.trim(model_name) {
        "" -> Error("Model name cannot be empty")
        trimmed -> Ok(trimmed)
      }
    }
    Error(_) -> Error("Invalid request: expected {\"model\": \"model_name\"}")
  }
}

/// Perform streaming pull and send progress via WebSocket
fn do_streaming_pull(conn: WebsocketConnection, model_name: String) -> Nil {
  let ollama_host = config.get_ollama_host()
  let litellm_host = config.get_litellm_host()
  let litellm_key = config.get_litellm_key()

  // Start streaming pull - returns a Subject that receives progress events
  case pull_stream.start_pull(ollama_host, model_name) {
    Ok(#(event_subject, _client_ref, _manager_subject)) -> {
      // Process events from the stream
      let pull_success = receive_pull_events(conn, event_subject, False)

      // Handle completion
      case pull_success {
        True -> {
          // Send pull success step
          let _ = mist.send_text_frame(
            conn,
            pull_stream.step_to_json("pull", True, "Model pulled successfully"),
          )

          // Register in LiteLLM
          let register_result = models.register_model_in_litellm(
            litellm_host,
            litellm_key,
            ollama_host,
            model_name,
          )

          let _ = mist.send_text_frame(
            conn,
            pull_stream.step_to_json("register", register_result.success, register_result.message),
          )

          // Send complete message
          let _ = mist.send_text_frame(
            conn,
            pull_stream.complete_to_json(True, "Model '" <> model_name <> "' added successfully!"),
          )
          Nil
        }
        False -> {
          let _ = mist.send_text_frame(
            conn,
            pull_stream.step_to_json("pull", False, "Model pull failed"),
          )
          let _ = mist.send_text_frame(
            conn,
            pull_stream.complete_to_json(False, "Failed to pull model"),
          )
          Nil
        }
      }
    }
    Error(err) -> {
      // Failed to start pull
      let _ = mist.send_text_frame(
        conn,
        pull_stream.step_to_json("pull", False, "Failed to start pull: " <> err),
      )
      let _ = mist.send_text_frame(
        conn,
        pull_stream.complete_to_json(False, "Failed to pull model"),
      )
      Nil
    }
  }
}

/// Receive and forward pull events until stream closes
/// Returns True if pull was successful
fn receive_pull_events(
  conn: WebsocketConnection,
  event_subject: process.Subject(jsonl.JsonlEvent(pull_stream.PullProgress)),
  saw_success: Bool,
) -> Bool {
  // Wait up to 30 minutes for events (large models take time)
  case process.receive(event_subject, 1_800_000) {
    Ok(Line(progress)) -> {
      // Send progress to WebSocket
      let _ = mist.send_text_frame(conn, pull_stream.progress_to_json(progress))

      // Track if we saw a success status
      let success = saw_success || pull_stream.is_success(progress)
      receive_pull_events(conn, event_subject, success)
    }
    Ok(Closed) -> {
      // Stream ended
      saw_success
    }
    Error(_) -> {
      // Timeout
      False
    }
  }
}
