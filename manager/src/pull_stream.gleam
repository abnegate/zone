//// Streaming model pull using httpp library

import gleam/bytes_tree
import gleam/dynamic/decode
import gleam/erlang/process.{type Subject}
import gleam/http
import gleam/http/request
import gleam/json
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/uri
import httpp/hackney
import httpp/jsonl.{type JsonlEvent}

// =============================================================================
// Types
// =============================================================================

pub type PullProgress {
  PullProgress(
    status: String,
    digest: Option(String),
    total: Option(Int),
    completed: Option(Int),
  )
}

// =============================================================================
// Streaming Pull
// =============================================================================

/// Start streaming a model pull from Ollama
/// Returns a subject that receives PullProgress events
pub fn start_pull(
  ollama_host: String,
  model_name: String,
) -> Result(
  #(Subject(JsonlEvent(PullProgress)), hackney.ClientRef, Subject(jsonl.JsonlManagerMessage)),
  String,
) {
  let url = ollama_host <> "/api/pull"

  // Create the request
  let req_result = uri.parse(url)
    |> result.map(fn(u) {
      request.from_uri(u)
      |> result.map(fn(req) {
        req
        |> request.set_method(http.Post)
        |> request.set_header("content-type", "application/json")
        |> request.set_body(bytes_tree.from_string(
          json.object([
            #("name", json.string(model_name)),
            #("stream", json.bool(True)),
          ])
          |> json.to_string
        ))
      })
    })
    |> result.flatten

  case req_result {
    Ok(req) -> {
      // Create subject to receive events
      let event_subject = process.new_subject()

      // Create decoder for Ollama progress
      let decoder = progress_decoder()

      // Start streaming - 30 second timeout for initial response
      case jsonl.json_lines_stream(req, 30_000, decoder, event_subject) {
        Ok(#(client_ref, manager_subject)) -> {
          Ok(#(event_subject, client_ref, manager_subject))
        }
        Error(_) -> Error("Failed to start streaming request")
      }
    }
    Error(_) -> Error("Invalid Ollama URL")
  }
}

/// Decoder for Ollama pull progress
fn progress_decoder() -> decode.Decoder(PullProgress) {
  use status <- decode.field("status", decode.string)
  use digest <- decode.optional_field("digest", None, decode.string |> decode.map(Some))
  use total <- decode.optional_field("total", None, decode.int |> decode.map(Some))
  use completed <- decode.optional_field("completed", None, decode.int |> decode.map(Some))
  decode.success(PullProgress(status, digest, total, completed))
}

/// Check if the progress indicates success
pub fn is_success(progress: PullProgress) -> Bool {
  progress.status == "success"
}

// =============================================================================
// JSON Message Formatting (for WebSocket)
// =============================================================================

/// Convert progress to JSON for WebSocket
pub fn progress_to_json(progress: PullProgress) -> String {
  let percent = case progress.total, progress.completed {
    Some(total), Some(completed) if total > 0 -> Some(completed * 100 / total)
    _, _ -> None
  }

  json.object([
    #("type", json.string("progress")),
    #("status", json.string(progress.status)),
    #("percent", case percent {
      Some(p) -> json.int(p)
      None -> json.null()
    }),
    #("completed", case progress.completed {
      Some(c) -> json.int(c)
      None -> json.null()
    }),
    #("total", case progress.total {
      Some(t) -> json.int(t)
      None -> json.null()
    }),
  ])
  |> json.to_string
}

/// Create a step message for WebSocket
pub fn step_to_json(step: String, success: Bool, message: String) -> String {
  json.object([
    #("type", json.string("step")),
    #("step", json.string(step)),
    #("success", json.bool(success)),
    #("message", json.string(message)),
  ])
  |> json.to_string
}

/// Create a complete message for WebSocket
pub fn complete_to_json(success: Bool, message: String) -> String {
  json.object([
    #("type", json.string("complete")),
    #("success", json.bool(success)),
    #("message", json.string(message)),
  ])
  |> json.to_string
}

/// Create an error message for WebSocket
pub fn error_to_json(message: String) -> String {
  json.object([
    #("type", json.string("error")),
    #("message", json.string(message)),
  ])
  |> json.to_string
}
