/// LLM client for communicating with LiteLLM/Ollama
/// Handles chat completions for agent phases
import config
import gleam/dynamic/decode
import gleam/http
import gleam/http/request
import gleam/httpc
import gleam/json
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string

/// Message role for chat completions
pub type Role {
  System
  User
  Assistant
}

/// A chat message
pub type Message {
  Message(role: Role, content: String)
}

/// Chat completion request
pub type ChatRequest {
  ChatRequest(
    model: String,
    messages: List(Message),
    temperature: Option(Float),
    max_tokens: Option(Int),
  )
}

/// Chat completion response
pub type ChatResponse {
  ChatResponse(content: String, model: String, tokens_used: Int)
}

/// Error types for LLM operations
pub type LlmError {
  NetworkError(String)
  ApiError(Int, String)
  ParseError(String)
  TimeoutError
}

/// Convert Role to string for API
fn role_to_string(role: Role) -> String {
  case role {
    System -> "system"
    User -> "user"
    Assistant -> "assistant"
  }
}

/// Convert message to JSON
fn message_to_json(msg: Message) -> json.Json {
  json.object([
    #("role", json.string(role_to_string(msg.role))),
    #("content", json.string(msg.content)),
  ])
}

/// Build request body for chat completion
fn build_request_body(req: ChatRequest) -> String {
  let messages_json = json.array(req.messages, message_to_json)

  let base_fields = [
    #("model", json.string(req.model)),
    #("messages", messages_json),
  ]

  let fields = case req.temperature {
    Some(t) -> [#("temperature", json.float(t)), ..base_fields]
    None -> base_fields
  }

  let fields = case req.max_tokens {
    Some(m) -> [#("max_tokens", json.int(m)), ..fields]
    None -> fields
  }

  json.object(fields)
  |> json.to_string
}

/// Parse chat completion response
fn parse_response(body: String) -> Result(ChatResponse, LlmError) {
  let decoder = {
    use choices <- decode.field(
      "choices",
      decode.list({
        use message <- decode.field("message", {
          use content <- decode.field("content", decode.string)
          decode.success(content)
        })
        decode.success(message)
      }),
    )
    use model <- decode.optional_field("model", "", decode.string)
    use usage <- decode.optional_field("usage", 0, {
      use total <- decode.optional_field("total_tokens", 0, decode.int)
      decode.success(total)
    })
    decode.success(#(choices, model, usage))
  }

  case json.parse(body, decoder) {
    Ok(#(choices, model, tokens)) -> {
      case choices {
        [content, ..] -> Ok(ChatResponse(content, model, tokens))
        [] -> Error(ParseError("No choices in response"))
      }
    }
    Error(e) ->
      Error(ParseError("Failed to parse response: " <> string.inspect(e)))
  }
}

/// Send a chat completion request to LiteLLM
pub fn chat(req: ChatRequest) -> Result(ChatResponse, LlmError) {
  let litellm_host = config.get_litellm_host()
  let litellm_key = config.get_litellm_key()
  let url = litellm_host <> "/v1/chat/completions"

  let body = build_request_body(req)

  case request.to(url) {
    Ok(base_req) -> {
      let http_req =
        base_req
        |> request.set_method(http.Post)
        |> request.set_body(body)
        |> request.set_header("content-type", "application/json")
        |> request.set_header("authorization", "Bearer " <> litellm_key)

      // Use longer timeout for LLM requests (5 minutes)
      case httpc.send(http_req) {
        Ok(resp) -> {
          case resp.status {
            200 -> parse_response(resp.body)
            status -> Error(ApiError(status, resp.body))
          }
        }
        Error(_) -> Error(NetworkError("Failed to connect to LiteLLM"))
      }
    }
    Error(_) -> Error(NetworkError("Invalid LiteLLM URL: " <> url))
  }
}

/// Convenience function for single-turn chat
pub fn complete(
  model: String,
  system_prompt: String,
  user_message: String,
) -> Result(String, LlmError) {
  let req =
    ChatRequest(
      model: model,
      messages: [
        Message(System, system_prompt),
        Message(User, user_message),
      ],
      temperature: Some(0.7),
      max_tokens: Some(4096),
    )

  chat(req)
  |> result.map(fn(resp) { resp.content })
}

/// Convenience function for continuing a conversation
pub fn continue_chat(
  model: String,
  system_prompt: String,
  history: List(Message),
  user_message: String,
) -> Result(String, LlmError) {
  let messages = [Message(System, system_prompt), ..history]
  let messages = list_append(messages, [Message(User, user_message)])

  let req =
    ChatRequest(
      model: model,
      messages: messages,
      temperature: Some(0.7),
      max_tokens: Some(4096),
    )

  chat(req)
  |> result.map(fn(resp) { resp.content })
}

/// Helper to append lists
fn list_append(a: List(a), b: List(a)) -> List(a) {
  case a {
    [] -> b
    [head, ..tail] -> [head, ..list_append(tail, b)]
  }
}

/// Convert LlmError to string for logging
pub fn error_to_string(error: LlmError) -> String {
  case error {
    NetworkError(msg) -> "Network error: " <> msg
    ApiError(status, body) ->
      "API error (" <> string.inspect(status) <> "): " <> body
    ParseError(msg) -> "Parse error: " <> msg
    TimeoutError -> "Request timed out"
  }
}
