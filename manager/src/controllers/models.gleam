import config
import gleam/dynamic/decode
import gleam/http
import gleam/int
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/string
import gleam/uri
import ollama_models
import services/http_client
import wisp.{type Request, type Response}

/// Handle /api/models routes
pub fn handle_models(req: Request) -> Response {
  case req.method {
    http.Get -> {
      let query = wisp.get_query(req)
      let source = get_query_param(query, "source", "installed")

      case source {
        "installed" -> list_installed_models()
        "ollama" -> {
          let search = get_query_param(query, "q", "")
          let limit = get_query_param(query, "limit", "20")
          let offset = get_query_param(query, "offset", "0")
          browse_ollama_library(search, limit, offset)
        }
        "huggingface" -> {
          let search = get_query_param(query, "q", "")
          let limit = get_query_param(query, "limit", "20")
          let offset = get_query_param(query, "offset", "0")
          browse_huggingface(search, limit, offset)
        }
        _ -> json_error_response(400, "Unknown source: " <> source)
      }
    }
    _ -> wisp.method_not_allowed([http.Get])
  }
}

/// Handle /api/models/:name routes
pub fn handle_model_by_name(req: Request, model_name: String) -> Response {
  case req.method {
    http.Get -> fetch_huggingface_model_info(model_name)
    http.Delete -> delete_model(model_name)
    _ -> wisp.method_not_allowed([http.Get, http.Delete])
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

  case http_client.delete(url, payload, []) {
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

      case http_client.post(url, payload, headers) {
        Ok(_) -> StepResult("litellm", True, "Deleted from LiteLLM")
        Error(_) -> StepResult("litellm", False, "Model may not exist in LiteLLM")
      }
    }
  }
}

// =============================================================================
// Browse Ollama Library
// =============================================================================

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

// =============================================================================
// Browse HuggingFace
// =============================================================================

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

  case http_client.get(url, []) {
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

fn transform_huggingface_response(body: String, limit: Int) -> String {
  // Parse the HuggingFace response and transform to our format
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
// HuggingFace Model Info
// =============================================================================

fn fetch_huggingface_model_info(model_id: String) -> Response {
  // Fetch model info from HuggingFace API (includes gguf size)
  let api_url = "https://huggingface.co/api/models/" <> model_id
  let readme_url = "https://huggingface.co/" <> model_id <> "/raw/main/README.md"

  // Get model info for size
  let gguf_size = case http_client.get(api_url, []) {
    Ok(api_body) -> parse_gguf_size(api_body)
    Error(_) -> None
  }

  // Get README content
  let content = case http_client.get(readme_url, []) {
    Ok(readme) -> Some(readme)
    Error(_) -> None
  }

  let response_json =
    json.object([
      #("success", json.bool(True)),
      #("content", case content {
        Some(c) -> json.string(c)
        None -> json.null()
      }),
      #("gguf_size", case gguf_size {
        Some(size) -> json.int(size)
        None -> json.null()
      }),
    ])
    |> json.to_string

  wisp.response(200)
  |> wisp.set_header("content-type", "application/json")
  |> wisp.string_body(response_json)
}

fn parse_gguf_size(body: String) -> Option(Int) {
  let decoder = {
    use gguf <- decode.optional_field("gguf", None, {
      use total <- decode.field("total", decode.int)
      decode.success(total)
    } |> decode.map(Some))
    decode.success(gguf)
  }

  case json.parse(body, decoder) {
    Ok(Some(size)) -> Some(size)
    _ -> None
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

      case http_client.post(url, payload, headers) {
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

fn uri_encode(s: String) -> String {
  uri.percent_encode(s)
}
