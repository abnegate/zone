import gleam/json
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import models/task

// =============================================================================
// TaskStatus conversion tests
// =============================================================================

pub fn task_status_to_string_created_test() {
  task.status_to_string(task.Created)
  |> should.equal("created")
}

pub fn task_status_to_string_queued_test() {
  task.status_to_string(task.Queued)
  |> should.equal("queued")
}

pub fn task_status_to_string_in_progress_test() {
  task.status_to_string(task.InProgress)
  |> should.equal("in_progress")
}

pub fn task_status_to_string_review_test() {
  task.status_to_string(task.Review)
  |> should.equal("review")
}

pub fn task_status_to_string_complete_test() {
  task.status_to_string(task.Complete)
  |> should.equal("complete")
}

pub fn task_status_to_string_blocked_test() {
  task.status_to_string(task.Blocked)
  |> should.equal("blocked")
}

pub fn task_status_from_string_created_test() {
  task.status_from_string("created")
  |> should.be_ok()
  |> should.equal(task.Created)
}

pub fn task_status_from_string_queued_test() {
  task.status_from_string("queued")
  |> should.be_ok()
  |> should.equal(task.Queued)
}

pub fn task_status_from_string_in_progress_test() {
  task.status_from_string("in_progress")
  |> should.be_ok()
  |> should.equal(task.InProgress)
}

pub fn task_status_from_string_review_test() {
  task.status_from_string("review")
  |> should.be_ok()
  |> should.equal(task.Review)
}

pub fn task_status_from_string_complete_test() {
  task.status_from_string("complete")
  |> should.be_ok()
  |> should.equal(task.Complete)
}

pub fn task_status_from_string_blocked_test() {
  task.status_from_string("blocked")
  |> should.be_ok()
  |> should.equal(task.Blocked)
}

pub fn task_status_from_string_invalid_test() {
  task.status_from_string("invalid")
  |> should.be_error()
}

// =============================================================================
// TaskRunStatus conversion tests
// =============================================================================

pub fn run_status_to_string_running_test() {
  task.run_status_to_string(task.Running)
  |> should.equal("running")
}

pub fn run_status_to_string_completed_test() {
  task.run_status_to_string(task.Completed)
  |> should.equal("completed")
}

pub fn run_status_to_string_failed_test() {
  task.run_status_to_string(task.Failed)
  |> should.equal("failed")
}

pub fn run_status_to_string_cancelled_test() {
  task.run_status_to_string(task.Cancelled)
  |> should.equal("cancelled")
}

pub fn run_status_from_string_running_test() {
  task.run_status_from_string("running")
  |> should.be_ok()
  |> should.equal(task.Running)
}

pub fn run_status_from_string_completed_test() {
  task.run_status_from_string("completed")
  |> should.be_ok()
  |> should.equal(task.Completed)
}

pub fn run_status_from_string_failed_test() {
  task.run_status_from_string("failed")
  |> should.be_ok()
  |> should.equal(task.Failed)
}

pub fn run_status_from_string_cancelled_test() {
  task.run_status_from_string("cancelled")
  |> should.be_ok()
  |> should.equal(task.Cancelled)
}

pub fn run_status_from_string_invalid_test() {
  task.run_status_from_string("invalid")
  |> should.be_error()
}

// =============================================================================
// LogLevel conversion tests
// =============================================================================

pub fn log_level_to_string_debug_test() {
  task.log_level_to_string(task.LogDebug)
  |> should.equal("debug")
}

pub fn log_level_to_string_info_test() {
  task.log_level_to_string(task.LogInfo)
  |> should.equal("info")
}

pub fn log_level_to_string_warning_test() {
  task.log_level_to_string(task.LogWarning)
  |> should.equal("warning")
}

pub fn log_level_to_string_error_test() {
  task.log_level_to_string(task.LogError)
  |> should.equal("error")
}

pub fn log_level_from_string_debug_test() {
  task.log_level_from_string("debug")
  |> should.be_ok()
  |> should.equal(task.LogDebug)
}

pub fn log_level_from_string_info_test() {
  task.log_level_from_string("info")
  |> should.be_ok()
  |> should.equal(task.LogInfo)
}

pub fn log_level_from_string_warning_test() {
  task.log_level_from_string("warning")
  |> should.be_ok()
  |> should.equal(task.LogWarning)
}

pub fn log_level_from_string_error_test() {
  task.log_level_from_string("error")
  |> should.be_ok()
  |> should.equal(task.LogError)
}

pub fn log_level_from_string_invalid_test() {
  task.log_level_from_string("invalid")
  |> should.be_error()
}

// =============================================================================
// Task to_json tests
// =============================================================================

pub fn task_to_json_test() {
  let t =
    task.Task(
      id: "task-id",
      project_id: "project-id",
      title: "Test Task",
      description: "A test task description",
      acceptance_criteria: Some("Must pass tests"),
      status: task.Created,
      priority: 3,
      model_name: Some("llama3.2"),
      dependencies: ["dep-1", "dep-2"],
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-01T00:00:00Z",
      started_at: None,
      completed_at: None,
    )

  let json_str =
    task.to_json(t)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"id\":\"task-id\""))
  should.be_true(string.contains(json_str, "\"project_id\":\"project-id\""))
  should.be_true(string.contains(json_str, "\"title\":\"Test Task\""))
  should.be_true(string.contains(json_str, "\"status\":\"created\""))
  should.be_true(string.contains(json_str, "\"priority\":3"))
  should.be_true(string.contains(json_str, "\"model_name\":\"llama3.2\""))
}

pub fn task_run_to_json_test() {
  let run =
    task.TaskRun(
      id: "run-id",
      task_id: "task-id",
      status: task.Running,
      current_phase: Some("architect"),
      progress_percent: 25,
      started_at: "2025-01-01T00:00:00Z",
      completed_at: None,
      error_message: None,
    )

  let json_str =
    task.run_to_json(run)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"id\":\"run-id\""))
  should.be_true(string.contains(json_str, "\"status\":\"running\""))
  should.be_true(string.contains(json_str, "\"current_phase\":\"architect\""))
  should.be_true(string.contains(json_str, "\"progress_percent\":25"))
}

pub fn task_run_log_to_json_test() {
  let log =
    task.TaskRunLog(
      id: "log-id",
      task_run_id: "run-id",
      phase: "developer_impl",
      agent_type: "developer",
      log_level: task.LogInfo,
      message: "Implementing feature",
      created_at: "2025-01-01T00:00:00Z",
    )

  let json_str =
    task.log_to_json(log)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"id\":\"log-id\""))
  should.be_true(string.contains(json_str, "\"phase\":\"developer_impl\""))
  should.be_true(string.contains(json_str, "\"agent_type\":\"developer\""))
  should.be_true(string.contains(json_str, "\"log_level\":\"info\""))
}
