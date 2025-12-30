import envoy
import gleam/dynamic/decode
import gleam/erlang/process
import gleam/http
import gleam/http/request.{type Request as HttpRequest}
import gleam/int
import gleam/http/response
import gleam/httpc
import gleam/json
import gleam/list
import gleam/option.{None, Some}
import gleam/result
import gleam/string
import gleam/uri
import httpp/jsonl.{Closed, Line}
import mist.{type Connection, type ResponseData, type WebsocketConnection, type WebsocketMessage}
import ollama_models
import pull_stream
import simplifile
import wisp.{type Request, type Response}
import wisp/wisp_mist

pub fn main() {
  wisp.configure_logger()

  let secret = wisp.random_string(64)

  let assert Ok(_) =
    mist.new(fn(req) { handle_mist_request(req, secret) })
    |> mist.bind("0.0.0.0")
    |> mist.port(8000)
    |> mist.start

  process.sleep_forever()
}

fn handle_mist_request(
  req: HttpRequest(Connection),
  secret: String,
) -> response.Response(ResponseData) {
  case request.path_segments(req) {
    ["ws", "pull"] -> handle_websocket_upgrade_with_auth(req)
    _ -> wisp_mist.handler(handle_request, secret)(req)
  }
}

fn handle_websocket_upgrade_with_auth(
  req: HttpRequest(Connection),
) -> response.Response(ResponseData) {
  let expected_key = get_manager_api_key()

  case expected_key {
    "" -> {
      // No API key configured - reject
      response.new(401)
      |> response.set_body(mist.Bytes(<<"Unauthorized: API key not configured":utf8>>))
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
          |> response.set_body(mist.Bytes(<<"Unauthorized: Invalid API key":utf8>>))
        }
      }
    }
  }
}

fn parse_query_params(query: String) -> List(#(String, String)) {
  query
  |> string.split("&")
  |> list.filter(fn(p) { p != "" })
  |> list.filter_map(fn(param) {
    case string.split_once(param, "=") {
      Ok(#(key, value)) ->  Ok(#(key, uri.percent_decode(value) |> result.unwrap(value)))
      Error(_) -> Ok(#(param, ""))
    }
  })
}

type WsState {
  WsState(conn: WebsocketConnection)
}

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

fn parse_ws_pull_request(text: String) -> Result(String, String) {
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

fn do_streaming_pull(conn: WebsocketConnection, model_name: String) -> Nil {
  let ollama_host = get_ollama_host()
  let litellm_host = get_litellm_host()
  let litellm_key = get_litellm_key()

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
          let register_result = register_model_in_litellm(
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

// =============================================================================
// Request Router
// =============================================================================

pub fn handle_request(req: Request) -> Response {
  // SECURITY MODEL for Manager API:
  // - Primary protection: API key authentication (Bearer token or X-API-Key header)
  // - Secondary protection: Network isolation (accessible only via voiz_edge network)
  // - Tertiary protection: Path traversal validation on static file serving
  //
  // All API endpoints require valid API key authentication.
  // The index page injects the API key for frontend use.
  // Static files (CSS/JS) are served without auth.

  use <- wisp.log_request(req)
  use <- wisp.rescue_crashes
  use req <- wisp.handle_head(req)

  case wisp.path_segments(req) {
    // Index page - no auth (frontend handles login UI)
    [] -> serve_index()
    // Static files - no auth (CSS/JS are public)
    ["static", ..rest] -> serve_static(rest)
    // API endpoints - all require API key auth
    ["api", ..rest] -> {
      case validate_api_key(req) {
        True -> route_api(req, rest)
        False -> unauthorized_response()
      }
    }
    _ -> wisp.not_found()
  }
}

fn route_api(req: Request, path: List(String)) -> Response {
  case path {
    ["models"] -> handle_models(req)
    ["models", model_name] -> handle_model_by_name(req, model_name)
    ["browse"] -> handle_browse(req)
    ["model-card", ..rest] -> handle_model_card(req, rest)
    _ -> wisp.not_found()
  }
}

fn validate_api_key(req: Request) -> Bool {
  let expected_key = get_manager_api_key()

  // If no API key is configured, reject all requests
  case expected_key {
    "" -> False
    key -> {
      // Check Authorization: Bearer <key> header
      let bearer_valid = case wisp.get_header(req, "authorization") {
        Some(auth) -> {
          case string.starts_with(auth, "Bearer ") {
            True -> string.drop_start(auth, 7) == key
            False -> False
          }
        }
        None -> False
      }

      // Check X-API-Key header as fallback
      let api_key_valid = case wisp.get_header(req, "x-api-key") {
        Some(provided_key) -> provided_key == key
        None -> False
      }

      bearer_valid || api_key_valid
    }
  }
}

fn unauthorized_response() -> Response {
  let error_json =
    json.object([
      #("success", json.bool(False)),
      #("error", json.string("Unauthorized: Invalid or missing API key")),
    ])
    |> json.to_string

  wisp.response(401)
  |> wisp.set_header("content-type", "application/json")
  |> wisp.set_header("www-authenticate", "Bearer")
  |> wisp.string_body(error_json)
}

// =============================================================================
// Static File Serving
// =============================================================================

fn serve_index() -> Response {
  case simplifile.read("templates/index.html") {
    Ok(content) ->
      wisp.response(200)
      |> wisp.set_header("content-type", "text/html; charset=utf-8")
      |> wisp.string_body(content)
    Error(_) -> wisp.not_found()
  }
}

fn serve_static(path: List(String)) -> Response {
  // Validate path segments to prevent directory traversal attacks
  case validate_path_segments(path) {
    False -> wisp.bad_request()
    True -> {
      let file_path = "static/" <> string.join(path, "/")

      case simplifile.read(file_path) {
        Ok(content) -> {
          let content_type = get_content_type(file_path)

          wisp.response(200)
          |> wisp.set_header("content-type", content_type)
          |> wisp.string_body(content)
        }
        Error(_) -> wisp.not_found()
      }
    }
  }
}

fn validate_path_segments(segments: List(String)) -> Bool {
  // Reject any segment containing "..", ".", or starting with "/"
  // This prevents path traversal attacks like ../../../etc/passwd
  list.all(segments, fn(segment) {
    !string.contains(segment, "..") &&
    segment != "." &&
    segment != "" &&
    !string.starts_with(segment, "/")
  })
}

fn get_content_type(path: String) -> String {
  let types = [
    #(".css", "text/css; charset=utf-8"),
    #(".js", "application/javascript; charset=utf-8"),
    #(".json", "application/json"),
    #(".svg", "image/svg+xml"),
  ]

  case list.find(types, fn(t) { string.ends_with(path, t.0) }) {
    Ok(t) -> t.1
    Error(_) -> "text/plain"
  }
}

// =============================================================================
// Environment Helpers
// =============================================================================

fn get_env(key: String, default: String) -> String {
  case envoy.get(key) {
    Ok(value) -> value
    Error(_) -> default
  }
}

fn get_ollama_host() -> String {
  get_env("OLLAMA_HOST", "http://ollama:11434")
}

fn get_litellm_host() -> String {
  get_env("LITELLM_HOST", "http://litellm:4000")
}

fn get_litellm_key() -> String {
  get_env("SECURITY_LITELLM_MASTER_KEY", "")
}

fn get_manager_api_key() -> String {
  get_env("SECURITY_MANAGER_API_KEY", "")
}

// =============================================================================
// API: List/Manage Models
// =============================================================================

fn handle_models(req: Request) -> Response {
  case req.method {
    http.Get -> list_models()
    _ -> wisp.method_not_allowed([http.Get])
  }
}

fn list_models() -> Response {
  let ollama_host = get_ollama_host()
  let url = ollama_host <> "/api/tags"

  case make_get_request(url, []) {
    Ok(body) -> {
      wisp.response(200)
      |> wisp.set_header("content-type", "application/json")
      |> wisp.string_body(body)
    }
    Error(err) -> {
      json_error_response(500, err)
    }
  }
}

fn handle_model_by_name(req: Request, model_name: String) -> Response {
  case req.method {
    http.Delete -> delete_model(model_name)
    _ -> wisp.method_not_allowed([http.Delete])
  }
}

fn delete_model(model_name: String) -> Response {
  let ollama_host = get_ollama_host()
  let litellm_host = get_litellm_host()
  let litellm_key = get_litellm_key()

  // Decode URL-encoded model name (e.g., "llama3.1%3A8b" -> "llama3.1:8b")
  let decoded_name = case uri.percent_decode(model_name) {
    Ok(name) -> name
    Error(_) -> model_name
  }

  // Step 1: Delete from Ollama
  let ollama_result = delete_from_ollama(ollama_host, decoded_name)

  // Step 2: Delete from LiteLLM (best effort)
  let litellm_result = delete_from_litellm(litellm_host, litellm_key, decoded_name)

  let response_json =
    json.object([
      #("success", json.bool(ollama_result.success)),
      #("message", json.string(ollama_result.message)),
      #("steps", json.array(
        [ollama_result, litellm_result],
        fn(step) {
          json.object([
            #("step", json.string(step.step)),
            #("success", json.bool(step.success)),
            #("message", json.string(step.message)),
          ])
        },
      )),
    ])
    |> json.to_string

  let status = case ollama_result.success {
    True -> 200
    False -> 500
  }

  wisp.response(status)
  |> wisp.set_header("content-type", "application/json")
  |> wisp.string_body(response_json)
}

fn delete_from_ollama(ollama_host: String, model_name: String) -> StepResult {
  let url = ollama_host <> "/api/delete"
  let payload =
    json.object([#("name", json.string(model_name))])
    |> json.to_string

  case make_delete_request(url, payload, []) {
    Ok(_) -> StepResult("ollama", True, "Deleted from Ollama")
    Error(err) -> StepResult("ollama", False, "Failed to delete from Ollama: " <> err)
  }
}

fn delete_from_litellm(
  litellm_host: String,
  litellm_key: String,
  model_name: String,
) -> StepResult {
  case litellm_key {
    "" -> StepResult("litellm", False, "LiteLLM key not configured")
    key -> {
      let url = litellm_host <> "/model/delete"
      let payload =
        json.object([#("id", json.string(model_name))])
        |> json.to_string

      let headers = [
        #("Authorization", "Bearer " <> key),
        #("x-api-key", key),
      ]

      case make_post_request(url, payload, headers) {
        Ok(_) -> StepResult("litellm", True, "Deleted from LiteLLM")
        Error(_) -> StepResult("litellm", False, "Model may not exist in LiteLLM")
      }
    }
  }
}

// =============================================================================
// API: Browse External Model Sources
// =============================================================================

fn handle_browse(req: Request) -> Response {
  case req.method {
    http.Get -> {
      let query = wisp.get_query(req)

      let source = get_query_param(query, "source", "huggingface")
      let search = get_query_param(query, "q", "")
      let limit = get_query_param(query, "limit", "20")
      let offset = get_query_param(query, "offset", "0")

      case source {
        "huggingface" -> browse_huggingface(search, limit, offset)
        "ollama" -> browse_ollama_library(search, limit, offset)
        _ -> json_error_response(400, "Unknown source: " <> source)
      }
    }
    _ -> wisp.method_not_allowed([http.Get])
  }
}

fn get_query_param(
  query: List(#(String, String)),
  key: String,
  default: String,
) -> String {
  case list.find(query, fn(pair) { pair.0 == key }) {
    Ok(pair) -> pair.1
    Error(_) -> default
  }
}

fn browse_huggingface(search: String, limit: String, offset: String) -> Response {
  // HuggingFace API for GGUF models (Ollama-compatible)
  let base_url = "https://huggingface.co/api/models"
  let query_params = case search {
    "" -> "?filter=gguf&sort=downloads&direction=-1&limit=" <> limit <> "&skip=" <> offset
    q -> "?search=" <> uri_encode(q) <> "&filter=gguf&sort=downloads&direction=-1&limit=" <> limit <> "&skip=" <> offset
  }
  let url = base_url <> query_params

  let limit_int = case int.parse(limit) {
    Ok(n) -> n
    Error(_) -> 20
  }

  case make_get_request(url, []) {
    Ok(body) -> {
      // Transform HuggingFace response to our format
      let transformed = transform_huggingface_response(body, limit_int)

      wisp.response(200)
      |> wisp.set_header("content-type", "application/json")
      |> wisp.string_body(transformed)
    }
    Error(err) -> {
      json_error_response(500, "Failed to fetch from HuggingFace: " <> err)
    }
  }
}

fn browse_ollama_library(search: String, limit_str: String, offset_str: String) -> Response {
  // Get all models from the ollama_models module
  let models = ollama_models.get_all_models()

  let filtered = case search {
    "" -> models
    q -> list.filter(models, fn(m: ollama_models.OllamaModel) {
      string.contains(string.lowercase(m.name), string.lowercase(q)) ||
      string.contains(string.lowercase(m.description), string.lowercase(q)) ||
      list.any(m.tags, fn(tag) { string.contains(string.lowercase(tag), string.lowercase(q)) })
    })
  }

  // Parse limit and offset with defaults
  let limit = case int.parse(limit_str) {
    Ok(n) -> n
    Error(_) -> 20
  }
  let offset = case int.parse(offset_str) {
    Ok(n) -> n
    Error(_) -> 0
  }

  // Apply pagination
  let total = list.length(filtered)
  let paginated = filtered
    |> list.drop(offset)
    |> list.take(limit)
  let has_more = offset + limit < total

  let models_json =
    json.object([
      #("source", json.string("ollama")),
      #("models", json.array(paginated, fn(m) {
        json.object([
          #("id", json.string(m.name)),
          #("name", json.string(m.name)),
          #("description", json.string(m.description)),
          #("downloads", json.int(m.pulls)),
          #("tags", json.array(m.tags, json.string)),
        ])
      })),
      #("total", json.int(total)),
      #("has_more", json.bool(has_more)),
    ])
    |> json.to_string

  wisp.response(200)
  |> wisp.set_header("content-type", "application/json")
  |> wisp.string_body(models_json)
}

fn transform_huggingface_response(body: String, limit: Int) -> String {
  // Parse the HuggingFace response and transform to our format
  // HuggingFace returns: [{id, modelId, downloads, tags, author, likes, lastModified, ...}, ...]

  let decoder = decode.list({
    use id <- decode.field("modelId", decode.string)
    use downloads <- decode.optional_field("downloads", 0, decode.int)
    use tags <- decode.optional_field("tags", [], decode.list(decode.string))
    use pipeline_tag <- decode.optional_field("pipeline_tag", "", decode.string)
    use author <- decode.optional_field("author", "", decode.string)
    use likes <- decode.optional_field("likes", 0, decode.int)
    use last_modified <- decode.optional_field("lastModified", "", decode.string)
    decode.success(#(id, downloads, tags, pipeline_tag, author, likes, last_modified))
  })

  case json.parse(body, decoder) {
    Ok(models) -> {
      // If we got exactly the limit, assume there are more results
      let has_more = list.length(models) >= limit

      let transformed =
        json.object([
          #("source", json.string("huggingface")),
          #("models", json.array(models, fn(m) {
            let #(id, downloads, tags, pipeline, author, likes, last_modified) = m

            json.object([
              #("id", json.string(id)),
              #("name", json.string(id)),
              #("description", json.string(case pipeline {
                "" -> "HuggingFace GGUF model"
                p -> "HuggingFace GGUF model - " <> p
              })),
              #("downloads", json.int(downloads)),
              #("tags", json.array(tags, json.string)),
              #("install_name", json.string("hf.co/" <> id)),
              #("author", json.string(author)),
              #("likes", json.int(likes)),
              #("last_modified", json.string(last_modified)),
              #("url", json.string("https://huggingface.co/" <> id)),
              #("pipeline_tag", json.string(pipeline)),
            ])
          })),
          #("has_more", json.bool(has_more)),
        ])
      json.to_string(transformed)
    }
    Error(_) -> {
      // Return empty result on parse error
      json.object([
        #("source", json.string("huggingface")),
        #("models", json.array([], fn(_: String) { json.null() })),
        #("has_more", json.bool(False)),
        #("error", json.string("Failed to parse HuggingFace response")),
      ])
      |> json.to_string
    }
  }
}

// =============================================================================
// API: Fetch Model Card (README)
// =============================================================================

fn handle_model_card(req: Request, path_segments: List(String)) -> Response {
  case req.method {
    http.Get -> {
      // Path segments contain the model ID (e.g., ["user", "model-name"])
      let model_id = string.join(path_segments, "/")
      fetch_huggingface_readme(model_id)
    }
    _ -> wisp.method_not_allowed([http.Get])
  }
}

fn fetch_huggingface_readme(model_id: String) -> Response {
  // Fetch README from HuggingFace
  let url = "https://huggingface.co/" <> model_id <> "/raw/main/README.md"

  case make_get_request(url, []) {
    Ok(content) -> {
      // Return the raw markdown content
      let response_json =
        json.object([
          #("success", json.bool(True)),
          #("content", json.string(content)),
        ])
        |> json.to_string

      wisp.response(200)
      |> wisp.set_header("content-type", "application/json")
      |> wisp.string_body(response_json)
    }
    Error(_) -> {
      // Try alternative path (some repos use different structure)
      json_error_response(404, "Model card not found")
    }
  }
}

// =============================================================================
// LiteLLM Registration
// =============================================================================

type StepResult {
  StepResult(step: String, success: Bool, message: String)
}

fn register_model_in_litellm(
  litellm_host: String,
  litellm_key: String,
  ollama_host: String,
  model_name: String,
) -> StepResult {
  case litellm_key {
    "" -> StepResult("register", False, "LiteLLM master key not configured")
    key -> {
      let url = litellm_host <> "/model/new"
      let payload =
        json.object([
          #("model_name", json.string(model_name)),
          #("litellm_params", json.object([
            #("model", json.string("ollama/" <> model_name)),
            #("api_base", json.string(ollama_host)),
            #("timeout", json.int(120)),
          ])),
          #("model_info", json.object([
            #("id", json.string(model_name)),
            #("mode", json.string("chat")),
          ])),
        ])
        |> json.to_string

      let headers = [
        #("Authorization", "Bearer " <> key),
        #("x-api-key", key),
      ]

      case make_post_request(url, payload, headers) {
        Ok(body) -> {
          case string.contains(body, "\"error\"") {
            True ->
              StepResult("register", False, "LiteLLM error: " <> body)
            False ->
              StepResult("register", True, "Model registered in LiteLLM")
          }
        }
        Error(err) ->
          StepResult("register", False, "Failed to register: " <> err)
      }
    }
  }
}

// =============================================================================
// HTTP Client Helpers
// =============================================================================

fn make_get_request(
  url: String,
  headers: List(#(String, String)),
) -> Result(String, String) {
  case request.to(url) {
    Ok(req) -> {
      let req_with_headers =
        list.fold(headers, req, fn(r, h) { request.set_header(r, h.0, h.1) })

      case httpc.send(req_with_headers) {
        Ok(response) -> Ok(response.body)
        Error(_) -> Error("HTTP request failed")
      }
    }
    Error(_) -> Error("Invalid URL: " <> url)
  }
}

fn make_post_request(
  url: String,
  body: String,
  headers: List(#(String, String)),
) -> Result(String, String) {
  case request.to(url) {
    Ok(req) -> {
      let req_with_method = request.set_method(req, http.Post)
      let req_with_body = request.set_body(req_with_method, body)
      let req_with_content_type =
        request.set_header(req_with_body, "content-type", "application/json")
      let req_with_headers =
        list.fold(headers, req_with_content_type, fn(r, h) {
          request.set_header(r, h.0, h.1)
        })

      case httpc.send(req_with_headers) {
        Ok(response) -> Ok(response.body)
        Error(_) -> Error("HTTP request failed")
      }
    }
    Error(_) -> Error("Invalid URL: " <> url)
  }
}

fn make_delete_request(
  url: String,
  body: String,
  headers: List(#(String, String)),
) -> Result(String, String) {
  case request.to(url) {
    Ok(req) -> {
      let req_with_method = request.set_method(req, http.Delete)
      let req_with_body = request.set_body(req_with_method, body)
      let req_with_content_type =
        request.set_header(req_with_body, "content-type", "application/json")
      let req_with_headers =
        list.fold(headers, req_with_content_type, fn(r, h) {
          request.set_header(r, h.0, h.1)
        })

      case httpc.send(req_with_headers) {
        Ok(response) -> Ok(response.body)
        Error(_) -> Error("HTTP request failed")
      }
    }
    Error(_) -> Error("Invalid URL: " <> url)
  }
}

// =============================================================================
// Utility Functions
// =============================================================================

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

fn uri_encode(s: String) -> String {
  case uri.percent_encode(s) {
    encoded -> encoded
  }
}
