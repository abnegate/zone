import agents/task_worker
import agents/task_worker/types.{
  ExecutionCompleted, ExecutionFailed, LogEntry, PhaseCompleted, PhaseStarted,
}
import gleam/dynamic/decode
import gleam/json
import gleam/string
import gleeunit
import gleeunit/should

pub fn main() {
  gleeunit.main()
}

// =============================================================================
// progress_to_json tests - PhaseStarted
// =============================================================================

pub fn progress_to_json_phase_started_has_correct_type_test() {
  let msg =
    PhaseStarted("run-123", "architect_planning", 10, "Starting Planning")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"type\":\"phase_started\"")
  |> should.be_true()
}

pub fn progress_to_json_phase_started_has_run_id_test() {
  let msg =
    PhaseStarted("run-123", "architect_planning", 10, "Starting Planning")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"run_id\":\"run-123\"")
  |> should.be_true()
}

pub fn progress_to_json_phase_started_has_phase_test() {
  let msg =
    PhaseStarted("run-123", "architect_planning", 10, "Starting Planning")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"phase\":\"architect_planning\"")
  |> should.be_true()
}

pub fn progress_to_json_phase_started_has_progress_percent_test() {
  let msg =
    PhaseStarted("run-123", "architect_planning", 10, "Starting Planning")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"progress_percent\":10")
  |> should.be_true()
}

pub fn progress_to_json_phase_started_has_message_test() {
  let msg =
    PhaseStarted("run-123", "architect_planning", 10, "Starting Planning")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"message\":\"Starting Planning\"")
  |> should.be_true()
}

// =============================================================================
// progress_to_json tests - PhaseCompleted
// =============================================================================

pub fn progress_to_json_phase_completed_has_correct_type_test() {
  let msg = PhaseCompleted("run-456", "developer_tests", 25, "Completed Tests")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"type\":\"phase_completed\"")
  |> should.be_true()
}

pub fn progress_to_json_phase_completed_has_run_id_test() {
  let msg = PhaseCompleted("run-456", "developer_tests", 25, "Completed Tests")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"run_id\":\"run-456\"")
  |> should.be_true()
}

pub fn progress_to_json_phase_completed_has_phase_test() {
  let msg = PhaseCompleted("run-456", "developer_tests", 25, "Completed Tests")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"phase\":\"developer_tests\"")
  |> should.be_true()
}

pub fn progress_to_json_phase_completed_has_progress_test() {
  let msg = PhaseCompleted("run-456", "developer_tests", 25, "Completed Tests")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"progress_percent\":25")
  |> should.be_true()
}

// =============================================================================
// progress_to_json tests - LogEntry
// =============================================================================

pub fn progress_to_json_log_entry_has_correct_type_test() {
  let msg =
    LogEntry(
      "run-789",
      "griller_review",
      "griller",
      "info",
      "Reviewing code...",
    )

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"type\":\"log\"")
  |> should.be_true()
}

pub fn progress_to_json_log_entry_has_run_id_test() {
  let msg =
    LogEntry(
      "run-789",
      "griller_review",
      "griller",
      "info",
      "Reviewing code...",
    )

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"run_id\":\"run-789\"")
  |> should.be_true()
}

pub fn progress_to_json_log_entry_has_phase_test() {
  let msg =
    LogEntry(
      "run-789",
      "griller_review",
      "griller",
      "info",
      "Reviewing code...",
    )

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"phase\":\"griller_review\"")
  |> should.be_true()
}

pub fn progress_to_json_log_entry_has_agent_type_test() {
  let msg =
    LogEntry(
      "run-789",
      "griller_review",
      "griller",
      "info",
      "Reviewing code...",
    )

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"agent_type\":\"griller\"")
  |> should.be_true()
}

pub fn progress_to_json_log_entry_has_log_level_test() {
  let msg =
    LogEntry(
      "run-789",
      "griller_review",
      "griller",
      "error",
      "Failed to review",
    )

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"log_level\":\"error\"")
  |> should.be_true()
}

pub fn progress_to_json_log_entry_has_message_test() {
  let msg =
    LogEntry(
      "run-789",
      "griller_review",
      "griller",
      "info",
      "Reviewing code...",
    )

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"message\":\"Reviewing code...\"")
  |> should.be_true()
}

// =============================================================================
// progress_to_json tests - ExecutionCompleted
// =============================================================================

pub fn progress_to_json_execution_completed_has_correct_type_test() {
  let msg = ExecutionCompleted("run-complete", True, "Task completed!")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"type\":\"complete\"")
  |> should.be_true()
}

pub fn progress_to_json_execution_completed_has_run_id_test() {
  let msg = ExecutionCompleted("run-complete", True, "Task completed!")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"run_id\":\"run-complete\"")
  |> should.be_true()
}

pub fn progress_to_json_execution_completed_success_true_test() {
  let msg = ExecutionCompleted("run-complete", True, "Task completed!")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"success\":true")
  |> should.be_true()
}

pub fn progress_to_json_execution_completed_success_false_test() {
  let msg = ExecutionCompleted("run-complete", False, "Task failed!")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"success\":false")
  |> should.be_true()
}

pub fn progress_to_json_execution_completed_has_message_test() {
  let msg = ExecutionCompleted("run-complete", True, "Task completed!")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"message\":\"Task completed!\"")
  |> should.be_true()
}

// =============================================================================
// progress_to_json tests - ExecutionFailed
// =============================================================================

pub fn progress_to_json_execution_failed_has_correct_type_test() {
  let msg = ExecutionFailed("run-failed", "LLM connection timeout")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"type\":\"error\"")
  |> should.be_true()
}

pub fn progress_to_json_execution_failed_has_run_id_test() {
  let msg = ExecutionFailed("run-failed", "LLM connection timeout")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"run_id\":\"run-failed\"")
  |> should.be_true()
}

pub fn progress_to_json_execution_failed_has_error_test() {
  let msg = ExecutionFailed("run-failed", "LLM connection timeout")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"error\":\"LLM connection timeout\"")
  |> should.be_true()
}

// =============================================================================
// Round-trip valid JSON tests
// =============================================================================

pub fn progress_to_json_phase_started_is_valid_json_test() {
  let msg = PhaseStarted("run-1", "phase", 50, "message")
  let json_str = task_worker.progress_to_json(msg)

  // Should be able to parse as valid JSON
  json.parse(json_str, decode.dynamic)
  |> should.be_ok()
}

pub fn progress_to_json_phase_completed_is_valid_json_test() {
  let msg = PhaseCompleted("run-1", "phase", 50, "message")
  let json_str = task_worker.progress_to_json(msg)

  json.parse(json_str, decode.dynamic)
  |> should.be_ok()
}

pub fn progress_to_json_log_entry_is_valid_json_test() {
  let msg = LogEntry("run-1", "phase", "agent", "info", "message")
  let json_str = task_worker.progress_to_json(msg)

  json.parse(json_str, decode.dynamic)
  |> should.be_ok()
}

pub fn progress_to_json_execution_completed_is_valid_json_test() {
  let msg = ExecutionCompleted("run-1", True, "message")
  let json_str = task_worker.progress_to_json(msg)

  json.parse(json_str, decode.dynamic)
  |> should.be_ok()
}

pub fn progress_to_json_execution_failed_is_valid_json_test() {
  let msg = ExecutionFailed("run-1", "error")
  let json_str = task_worker.progress_to_json(msg)

  json.parse(json_str, decode.dynamic)
  |> should.be_ok()
}

// =============================================================================
// Edge case tests - special characters in strings
// =============================================================================

pub fn progress_to_json_handles_quotes_in_message_test() {
  let msg = PhaseStarted("run", "phase", 0, "Message with \"quotes\"")

  let json_str = task_worker.progress_to_json(msg)
  // Escaped quotes should be in the JSON
  string.contains(json_str, "\\\"quotes\\\"")
  |> should.be_true()
}

pub fn progress_to_json_handles_newlines_in_message_test() {
  let msg = LogEntry("run", "phase", "agent", "info", "Line 1\nLine 2")

  let json_str = task_worker.progress_to_json(msg)
  // Newlines should be escaped in JSON
  string.contains(json_str, "\\n")
  |> should.be_true()
}

pub fn progress_to_json_handles_unicode_in_message_test() {
  let msg = ExecutionCompleted("run", True, "Task completed!")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "Task completed!")
  |> should.be_true()
}

pub fn progress_to_json_handles_empty_strings_test() {
  let msg = PhaseStarted("", "", 0, "")

  let json_str = task_worker.progress_to_json(msg)
  string.contains(json_str, "\"run_id\":\"\"")
  |> should.be_true()
}
