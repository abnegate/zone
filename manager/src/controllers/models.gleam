/// Model Controller
/// Handles /api/models routes for browsing and managing models
///
/// To add a new model source:
/// 1. Create a module in sources/ with browse() and get_info() functions
/// 2. Export a handler() function returning a ModelSourceHandler
/// 3. Add it to get_source_handler() below
import config
import gleam/http
import gleam/int
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/string
import gleam/uri
import services/http_client
import sources/huggingface
import sources/modelscope
import sources/ollama_library
import sources/source.{type ModelSourceHandler}
import wisp.{type Request, type Response}

// =============================================================================
// Source Registry - the ONLY place where sources are dispatched
// =============================================================================

/// Get a handler for a source by ID
/// This is the ONLY place where source types are dispatched
fn get_source_handler(source_id: String) -> Option(ModelSourceHandler) {
  case source_id {
    "ollama" -> Some(ollama_library.handler())
    "huggingface" -> Some(huggingface.handler())
    "modelscope" -> Some(modelscope.handler())
    _ -> None
  }
}

/// List of all available source IDs for browsing
pub const available_sources = ["ollama", "huggingface", "modelscope"]

// =============================================================================
// Route Handlers
// =============================================================================

/// Handle /api/models routes
pub fn handle_models(req: Request) -> Response {
  case req.method {
    http.Get -> {
      let query = wisp.get_query(req)
      let source_name = get_query_param(query, "source", "installed")

      case source_name {
        "installed" -> list_installed_models()
        source_id -> {
          case get_source_handler(source_id) {
            Some(handler) -> {
              let search = get_query_param(query, "q", "")
              let limit =
                parse_int_param(get_query_param(query, "limit", "20"), 20)
              let offset =
                parse_int_param(get_query_param(query, "offset", "0"), 0)
              browse_source(handler.browse(search, limit, offset))
            }
            None -> json_error_response(400, "Unknown source: " <> source_id)
          }
        }
      }
    }
    _ -> wisp.method_not_allowed([http.Get])
  }
}

/// Handle /api/models/:name routes
/// Model name may include source prefix: huggingface/author/model, modelscope/author/model
/// or just author/model (defaults to huggingface)
pub fn handle_model_by_name(req: Request, model_name: String) -> Response {
  case req.method {
    http.Get -> {
      // Parse source prefix from model name
      let #(source_id, actual_name) = parse_model_source(model_name)
      case get_source_handler(source_id) {
        Some(handler) -> get_model_info(handler.get_info(actual_name))
        None -> json_error_response(400, "Unknown source: " <> source_id)
      }
    }
    http.Delete -> delete_model(model_name)
    _ -> wisp.method_not_allowed([http.Get, http.Delete])
  }
}

/// Parse source prefix from model name
/// Returns (source_id, actual_model_name)
fn parse_model_source(model_name: String) -> #(String, String) {
  case model_name {
    "modelscope/" <> rest -> #("modelscope", rest)
    "huggingface/" <> rest -> #("huggingface", rest)
    "ollama/" <> rest -> #("ollama", rest)
    // Default to huggingface when no source prefix specified
    _ -> #("huggingface", model_name)
  }
}

// =============================================================================
// List Installed Models
// =============================================================================

fn list_installed_models() -> Response {
  let ollama_host = config.get_ollama_host()
  let url = ollama_host <> "/api/tags"

  case http_client.get(url, []) {
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

// =============================================================================
// Source Browse/Info Handlers
// =============================================================================

fn browse_source(result: Result(source.BrowseResult, String)) -> Response {
  case result {
    Ok(browse_result) -> {
      wisp.response(200)
      |> wisp.set_header("content-type", "application/json")
      |> wisp.string_body(source.browse_result_to_json(browse_result))
    }
    Error(err) -> json_error_response(500, err)
  }
}

fn get_model_info(result: Result(source.ModelInfo, String)) -> Response {
  case result {
    Ok(info) -> {
      wisp.response(200)
      |> wisp.set_header("content-type", "application/json")
      |> wisp.string_body(source.model_info_to_json(info))
    }
    Error(err) -> json_error_response(404, err)
  }
}

fn parse_int_param(value: String, default: Int) -> Int {
  case int.parse(value) {
    Ok(n) -> n
    Error(_) -> default
  }
}

// =============================================================================
// Delete Model
// =============================================================================

pub type StepResult {
  StepResult(step: String, success: Bool, message: String)
}

fn delete_model(model_name: String) -> Response {
  let ollama_host = config.get_ollama_host()
  let litellm_host = config.get_litellm_host()
  let litellm_key = config.get_litellm_key()

  // Decode URL-encoded model name (e.g., "llama3.1%3A8b" -> "llama3.1:8b")
  let decoded_name = case uri.percent_decode(model_name) {
    Ok(name) -> name
    Error(_) -> model_name
  }

  // Step 1: Delete from Ollama
  let ollama_result = delete_from_ollama(ollama_host, decoded_name)

  // Step 2: Delete from LiteLLM (best effort)
  let litellm_result =
    delete_from_litellm(litellm_host, litellm_key, decoded_name)

  let response_json =
    json.object([
      #("success", json.bool(ollama_result.success)),
      #("message", json.string(ollama_result.message)),
      #(
        "steps",
        json.array([ollama_result, litellm_result], fn(step) {
          json.object([
            #("step", json.string(step.step)),
            #("success", json.bool(step.success)),
            #("message", json.string(step.message)),
          ])
        }),
      ),
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

  case http_client.delete(url, payload, []) {
    Ok(_) -> StepResult("ollama", True, "Deleted from Ollama")
    Error(err) ->
      StepResult("ollama", False, "Failed to delete from Ollama: " <> err)
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

      case http_client.post(url, payload, headers) {
        Ok(_) -> StepResult("litellm", True, "Deleted from LiteLLM")
        Error(_) ->
          StepResult("litellm", False, "Model may not exist in LiteLLM")
      }
    }
  }
}

// =============================================================================
// LiteLLM Registration (exported for use by websocket handler)
// =============================================================================

pub fn register_model_in_litellm(
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
          #(
            "litellm_params",
            json.object([
              #("model", json.string("ollama/" <> model_name)),
              #("api_base", json.string(ollama_host)),
              #("timeout", json.int(120)),
            ]),
          ),
          #(
            "model_info",
            json.object([
              #("id", json.string(model_name)),
              #("mode", json.string("chat")),
            ]),
          ),
        ])
        |> json.to_string

      let headers = [
        #("Authorization", "Bearer " <> key),
        #("x-api-key", key),
      ]

      case http_client.post(url, payload, headers) {
        Ok(body) -> {
          case string.contains(body, "\"error\"") {
            True -> StepResult("register", False, "LiteLLM error: " <> body)
            False -> StepResult("register", True, "Model registered in LiteLLM")
          }
        }
        Error(err) ->
          StepResult("register", False, "Failed to register: " <> err)
      }
    }
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
  case list.find(query, fn(pair) { pair.0 == key }) {
    Ok(pair) -> pair.1
    Error(_) -> default
  }
}

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
