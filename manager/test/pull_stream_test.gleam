import gleam/dynamic/decode
import gleam/json
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import pull_stream.{PullProgress}

// =============================================================================
// is_success tests
// =============================================================================

pub fn is_success_true_test() {
  PullProgress(status: "success", digest: None, total: None, completed: None)
  |> pull_stream.is_success()
  |> should.be_true()
}

pub fn is_success_false_downloading_test() {
  PullProgress(status: "downloading", digest: Some("sha256:abc"), total: Some(1000), completed: Some(500))
  |> pull_stream.is_success()
  |> should.be_false()
}

pub fn is_success_false_pulling_test() {
  PullProgress(status: "pulling manifest", digest: None, total: None, completed: None)
  |> pull_stream.is_success()
  |> should.be_false()
}

pub fn is_success_false_verifying_test() {
  PullProgress(status: "verifying sha256 digest", digest: Some("sha256:abc"), total: None, completed: None)
  |> pull_stream.is_success()
  |> should.be_false()
}

// =============================================================================
// progress_to_json tests
// =============================================================================

pub fn progress_to_json_basic_test() {
  let progress = PullProgress(
    status: "downloading",
    digest: Some("sha256:abc123"),
    total: Some(1000),
    completed: Some(500),
  )

  let json_str = pull_stream.progress_to_json(progress)

  // Verify it contains expected fields
  json_str |> string.contains("\"type\":\"progress\"") |> should.be_true()
  json_str |> string.contains("\"status\":\"downloading\"") |> should.be_true()
  json_str |> string.contains("\"percent\":50") |> should.be_true()
  json_str |> string.contains("\"completed\":500") |> should.be_true()
  json_str |> string.contains("\"total\":1000") |> should.be_true()
}

pub fn progress_to_json_no_total_test() {
  let progress = PullProgress(
    status: "pulling manifest",
    digest: None,
    total: None,
    completed: None,
  )

  let json_str = pull_stream.progress_to_json(progress)

  json_str |> string.contains("\"type\":\"progress\"") |> should.be_true()
  json_str |> string.contains("\"status\":\"pulling manifest\"") |> should.be_true()
  json_str |> string.contains("\"percent\":null") |> should.be_true()
}

pub fn progress_to_json_zero_total_test() {
  // Edge case: total is 0, should not divide by zero
  let progress = PullProgress(
    status: "downloading",
    digest: Some("sha256:abc"),
    total: Some(0),
    completed: Some(0),
  )

  let json_str = pull_stream.progress_to_json(progress)

  // Should handle division by zero gracefully (percent should be null)
  json_str |> string.contains("\"percent\":null") |> should.be_true()
}

pub fn progress_to_json_100_percent_test() {
  let progress = PullProgress(
    status: "downloading",
    digest: Some("sha256:abc"),
    total: Some(1000),
    completed: Some(1000),
  )

  let json_str = pull_stream.progress_to_json(progress)

  json_str |> string.contains("\"percent\":100") |> should.be_true()
}

pub fn progress_to_json_large_numbers_test() {
  let progress = PullProgress(
    status: "downloading",
    digest: Some("sha256:abc123def456"),
    total: Some(5_000_000_000),
    completed: Some(2_500_000_000),
  )

  let json_str = pull_stream.progress_to_json(progress)

  json_str |> string.contains("\"percent\":50") |> should.be_true()
}

// =============================================================================
// step_to_json tests
// =============================================================================

pub fn step_to_json_success_test() {
  let json_str = pull_stream.step_to_json("pull", True, "Model pulled successfully")

  json_str |> string.contains("\"type\":\"step\"") |> should.be_true()
  json_str |> string.contains("\"step\":\"pull\"") |> should.be_true()
  json_str |> string.contains("\"success\":true") |> should.be_true()
  json_str |> string.contains("\"message\":\"Model pulled successfully\"") |> should.be_true()
}

pub fn step_to_json_failure_test() {
  let json_str = pull_stream.step_to_json("register", False, "Failed to register model")

  json_str |> string.contains("\"type\":\"step\"") |> should.be_true()
  json_str |> string.contains("\"step\":\"register\"") |> should.be_true()
  json_str |> string.contains("\"success\":false") |> should.be_true()
  json_str |> string.contains("\"message\":\"Failed to register model\"") |> should.be_true()
}

pub fn step_to_json_empty_message_test() {
  let json_str = pull_stream.step_to_json("test", True, "")

  json_str |> string.contains("\"message\":\"\"") |> should.be_true()
}

// =============================================================================
// complete_to_json tests
// =============================================================================

pub fn complete_to_json_success_test() {
  let json_str = pull_stream.complete_to_json(True, "Model 'llama3.1:8b' added successfully!")

  json_str |> string.contains("\"type\":\"complete\"") |> should.be_true()
  json_str |> string.contains("\"success\":true") |> should.be_true()
  json_str |> string.contains("Model 'llama3.1:8b' added successfully!") |> should.be_true()
}

pub fn complete_to_json_failure_test() {
  let json_str = pull_stream.complete_to_json(False, "Failed to pull model")

  json_str |> string.contains("\"type\":\"complete\"") |> should.be_true()
  json_str |> string.contains("\"success\":false") |> should.be_true()
  json_str |> string.contains("\"message\":\"Failed to pull model\"") |> should.be_true()
}

// =============================================================================
// error_to_json tests
// =============================================================================

pub fn error_to_json_basic_test() {
  let json_str = pull_stream.error_to_json("Something went wrong")

  json_str |> string.contains("\"type\":\"error\"") |> should.be_true()
  json_str |> string.contains("\"message\":\"Something went wrong\"") |> should.be_true()
}

pub fn error_to_json_empty_message_test() {
  let json_str = pull_stream.error_to_json("")

  json_str |> string.contains("\"type\":\"error\"") |> should.be_true()
  json_str |> string.contains("\"message\":\"\"") |> should.be_true()
}

pub fn error_to_json_special_chars_test() {
  let json_str = pull_stream.error_to_json("Error: \"invalid\" model <name>")

  json_str |> string.contains("\"type\":\"error\"") |> should.be_true()
  // JSON should properly escape the quotes
  json_str |> string.contains("\\\"invalid\\\"") |> should.be_true()
}

// =============================================================================
// JSON validity tests - ensure all outputs are valid JSON
// =============================================================================

pub fn progress_to_json_valid_json_test() {
  let progress = PullProgress(
    status: "downloading",
    digest: Some("sha256:abc"),
    total: Some(1000),
    completed: Some(500),
  )

  let json_str = pull_stream.progress_to_json(progress)

  // This should parse without error - we use decode.dynamic to just check validity
  let decoder = {
    use _ <- decode.field("type", decode.string)
    decode.success(Nil)
  }

  json.parse(json_str, decoder)
  |> should.be_ok()
}

pub fn step_to_json_valid_json_test() {
  let json_str = pull_stream.step_to_json("test", True, "test message")

  let decoder = {
    use _ <- decode.field("type", decode.string)
    decode.success(Nil)
  }

  json.parse(json_str, decoder)
  |> should.be_ok()
}

pub fn complete_to_json_valid_json_test() {
  let json_str = pull_stream.complete_to_json(True, "done")

  let decoder = {
    use _ <- decode.field("type", decode.string)
    decode.success(Nil)
  }

  json.parse(json_str, decoder)
  |> should.be_ok()
}

pub fn error_to_json_valid_json_test() {
  let json_str = pull_stream.error_to_json("error message")

  let decoder = {
    use _ <- decode.field("type", decode.string)
    decode.success(Nil)
  }

  json.parse(json_str, decoder)
  |> should.be_ok()
}
