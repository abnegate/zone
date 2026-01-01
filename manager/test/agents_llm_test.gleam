import agents/llm.{ApiError, NetworkError, ParseError, TimeoutError}
import gleam/string
import gleeunit
import gleeunit/should

pub fn main() {
  gleeunit.main()
}

// =============================================================================
// error_to_string tests
// =============================================================================

pub fn error_to_string_network_error_test() {
  let error = NetworkError("Connection refused")

  llm.error_to_string(error)
  |> should.equal("Network error: Connection refused")
}

pub fn error_to_string_network_error_empty_message_test() {
  let error = NetworkError("")

  llm.error_to_string(error)
  |> should.equal("Network error: ")
}

pub fn error_to_string_api_error_test() {
  let error = ApiError(500, "Internal Server Error")

  let result = llm.error_to_string(error)

  string.contains(result, "API error")
  |> should.be_true()

  string.contains(result, "500")
  |> should.be_true()

  string.contains(result, "Internal Server Error")
  |> should.be_true()
}

pub fn error_to_string_api_error_401_test() {
  let error = ApiError(401, "Unauthorized")

  let result = llm.error_to_string(error)

  string.contains(result, "401")
  |> should.be_true()

  string.contains(result, "Unauthorized")
  |> should.be_true()
}

pub fn error_to_string_api_error_404_test() {
  let error = ApiError(404, "Model not found")

  let result = llm.error_to_string(error)

  string.contains(result, "404")
  |> should.be_true()
}

pub fn error_to_string_api_error_429_rate_limit_test() {
  let error = ApiError(429, "Rate limit exceeded")

  let result = llm.error_to_string(error)

  string.contains(result, "429")
  |> should.be_true()

  string.contains(result, "Rate limit exceeded")
  |> should.be_true()
}

pub fn error_to_string_api_error_with_json_body_test() {
  let error = ApiError(400, "{\"error\": \"Invalid request\"}")

  let result = llm.error_to_string(error)

  string.contains(result, "400")
  |> should.be_true()

  string.contains(result, "Invalid request")
  |> should.be_true()
}

pub fn error_to_string_parse_error_test() {
  let error = ParseError("No choices in response")

  llm.error_to_string(error)
  |> should.equal("Parse error: No choices in response")
}

pub fn error_to_string_parse_error_json_error_test() {
  let error = ParseError("Failed to parse: unexpected token")

  let result = llm.error_to_string(error)

  string.contains(result, "Parse error")
  |> should.be_true()

  string.contains(result, "unexpected token")
  |> should.be_true()
}

pub fn error_to_string_timeout_error_test() {
  let error = TimeoutError

  llm.error_to_string(error)
  |> should.equal("Request timed out")
}

// =============================================================================
// Error type construction tests
// =============================================================================

pub fn network_error_preserves_message_test() {
  let msg = "Failed to connect to LiteLLM at http://localhost:4000"
  let error = NetworkError(msg)

  case error {
    NetworkError(m) -> should.equal(m, msg)
    _ -> should.fail()
  }
}

pub fn api_error_preserves_status_and_body_test() {
  let status = 503
  let body = "Service unavailable"
  let error = ApiError(status, body)

  case error {
    ApiError(s, b) -> {
      should.equal(s, status)
      should.equal(b, body)
    }
    _ -> should.fail()
  }
}

pub fn parse_error_preserves_message_test() {
  let msg = "JSON decode error at position 42"
  let error = ParseError(msg)

  case error {
    ParseError(m) -> should.equal(m, msg)
    _ -> should.fail()
  }
}

// =============================================================================
// Error message format tests
// =============================================================================

pub fn error_messages_are_human_readable_test() {
  // All error messages should not contain internal Gleam type syntax
  let errors = [
    NetworkError("test"),
    ApiError(500, "test"),
    ParseError("test"),
    TimeoutError,
  ]

  errors
  |> check_all_readable()
  |> should.be_true()
}

fn check_all_readable(errors: List(llm.LlmError)) -> Bool {
  case errors {
    [] -> True
    [error, ..rest] -> {
      let msg = llm.error_to_string(error)
      // Should not contain raw Gleam syntax markers
      case
        string.contains(msg, "#(")
        || string.contains(msg, "Ok(")
        || string.contains(msg, "Error(")
      {
        True -> False
        False -> check_all_readable(rest)
      }
    }
  }
}
