import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}

/// Task status enum
pub type TaskStatus {
  Created
  Queued
  InProgress
  Review
  Complete
  Blocked
}

/// Task entity
pub type Task {
  Task(
    id: String,
    project_id: String,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: TaskStatus,
    priority: Int,
    model_name: Option(String),
    dependencies: List(String),
    created_at: String,
    updated_at: String,
    started_at: Option(String),
    completed_at: Option(String),
    /// Whether this task uses agent tools (file read/write, KB search, etc.)
    is_agentic: Bool,
    /// Optional GitHub repo URL (overrides project's repo if set)
    github_repo_url: Option(String),
    /// When the task was added to the execution queue
    queued_at: Option(String),
    /// ID of the worker currently processing this task
    worker_id: Option(String),
  )
}

/// Task run entity
pub type TaskRun {
  TaskRun(
    id: String,
    task_id: String,
    status: TaskRunStatus,
    current_phase: Option(String),
    progress_percent: Int,
    started_at: String,
    completed_at: Option(String),
    error_message: Option(String),
  )
}

/// Task run status
pub type TaskRunStatus {
  Running
  Completed
  Failed
  Cancelled
}

/// Task run log entry
pub type TaskRunLog {
  TaskRunLog(
    id: String,
    task_run_id: String,
    phase: String,
    agent_type: String,
    log_level: LogLevel,
    message: String,
    created_at: String,
  )
}

/// Log level enum
pub type LogLevel {
  LogDebug
  LogInfo
  LogWarning
  LogError
}

/// Request to create a new task
pub type CreateTaskRequest {
  CreateTaskRequest(
    project_id: String,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(List(String)),
    /// Whether this task uses agent tools
    is_agentic: Option(Bool),
    /// Optional GitHub repo URL for agentic tasks
    github_repo_url: Option(String),
  )
}

/// Request to update a task
pub type UpdateTaskRequest {
  UpdateTaskRequest(
    title: Option(String),
    description: Option(String),
    acceptance_criteria: Option(String),
    status: Option(TaskStatus),
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(List(String)),
    /// Whether this task uses agent tools
    is_agentic: Option(Bool),
    /// Optional GitHub repo URL for agentic tasks
    github_repo_url: Option(String),
  )
}

/// Convert TaskStatus to string for database
pub fn status_to_string(status: TaskStatus) -> String {
  case status {
    Created -> "created"
    Queued -> "queued"
    InProgress -> "in_progress"
    Review -> "review"
    Complete -> "complete"
    Blocked -> "blocked"
  }
}

/// Parse string to TaskStatus
pub fn status_from_string(s: String) -> Result(TaskStatus, Nil) {
  case s {
    "created" -> Ok(Created)
    "queued" -> Ok(Queued)
    "in_progress" -> Ok(InProgress)
    "review" -> Ok(Review)
    "complete" -> Ok(Complete)
    "blocked" -> Ok(Blocked)
    _ -> Error(Nil)
  }
}

/// Convert TaskRunStatus to string
pub fn run_status_to_string(status: TaskRunStatus) -> String {
  case status {
    Running -> "running"
    Completed -> "completed"
    Failed -> "failed"
    Cancelled -> "cancelled"
  }
}

/// Parse string to TaskRunStatus
pub fn run_status_from_string(s: String) -> Result(TaskRunStatus, Nil) {
  case s {
    "running" -> Ok(Running)
    "completed" -> Ok(Completed)
    "failed" -> Ok(Failed)
    "cancelled" -> Ok(Cancelled)
    _ -> Error(Nil)
  }
}

/// Convert LogLevel to string
pub fn log_level_to_string(level: LogLevel) -> String {
  case level {
    LogDebug -> "debug"
    LogInfo -> "info"
    LogWarning -> "warning"
    LogError -> "error"
  }
}

/// Parse string to LogLevel
pub fn log_level_from_string(s: String) -> Result(LogLevel, Nil) {
  case s {
    "debug" -> Ok(LogDebug)
    "info" -> Ok(LogInfo)
    "warning" -> Ok(LogWarning)
    "error" -> Ok(LogError)
    _ -> Error(Nil)
  }
}

/// Decoder for TaskStatus from database string
pub fn status_decoder() -> decode.Decoder(TaskStatus) {
  decode.string
  |> decode.then(fn(s) {
    case status_from_string(s) {
      Ok(status) -> decode.success(status)
      Error(_) -> decode.failure(Created, "TaskStatus")
    }
  })
}

/// Decoder for TaskRunStatus from database string
pub fn run_status_decoder() -> decode.Decoder(TaskRunStatus) {
  decode.string
  |> decode.then(fn(s) {
    case run_status_from_string(s) {
      Ok(status) -> decode.success(status)
      Error(_) -> decode.failure(Running, "TaskRunStatus")
    }
  })
}

/// Decoder for LogLevel from database string
pub fn log_level_decoder() -> decode.Decoder(LogLevel) {
  decode.string
  |> decode.then(fn(s) {
    case log_level_from_string(s) {
      Ok(level) -> decode.success(level)
      Error(_) -> decode.failure(LogInfo, "LogLevel")
    }
  })
}

/// Convert Task to JSON
pub fn to_json(task: Task) -> json.Json {
  json.object([
    #("id", json.string(task.id)),
    #("project_id", json.string(task.project_id)),
    #("title", json.string(task.title)),
    #("description", json.string(task.description)),
    #("acceptance_criteria", option_to_json(task.acceptance_criteria)),
    #("status", json.string(status_to_string(task.status))),
    #("priority", json.int(task.priority)),
    #("model_name", option_to_json(task.model_name)),
    #("dependencies", json.array(task.dependencies, json.string)),
    #("created_at", json.string(task.created_at)),
    #("updated_at", json.string(task.updated_at)),
    #("started_at", option_to_json(task.started_at)),
    #("completed_at", option_to_json(task.completed_at)),
    #("is_agentic", json.bool(task.is_agentic)),
    #("github_repo_url", option_to_json(task.github_repo_url)),
    #("queued_at", option_to_json(task.queued_at)),
    #("worker_id", option_to_json(task.worker_id)),
  ])
}

/// Convert TaskRun to JSON
pub fn run_to_json(run: TaskRun) -> json.Json {
  json.object([
    #("id", json.string(run.id)),
    #("task_id", json.string(run.task_id)),
    #("status", json.string(run_status_to_string(run.status))),
    #("current_phase", option_to_json(run.current_phase)),
    #("progress_percent", json.int(run.progress_percent)),
    #("started_at", json.string(run.started_at)),
    #("completed_at", option_to_json(run.completed_at)),
    #("error_message", option_to_json(run.error_message)),
  ])
}

/// Convert TaskRunLog to JSON
pub fn log_to_json(log: TaskRunLog) -> json.Json {
  json.object([
    #("id", json.string(log.id)),
    #("task_run_id", json.string(log.task_run_id)),
    #("phase", json.string(log.phase)),
    #("agent_type", json.string(log.agent_type)),
    #("log_level", json.string(log_level_to_string(log.log_level))),
    #("message", json.string(log.message)),
    #("created_at", json.string(log.created_at)),
  ])
}

fn option_to_json(opt: Option(String)) -> json.Json {
  case opt {
    Some(s) -> json.string(s)
    None -> json.null()
  }
}
