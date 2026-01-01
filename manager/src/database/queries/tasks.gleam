import birl
import database/connection.{type Connection, query_error_to_string}
import gleam/dynamic/decode
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import models/task.{
  type CreateTaskRequest, type LogLevel, type Task, type TaskRun,
  type TaskRunLog, type TaskRunStatus, type TaskStatus, type UpdateTaskRequest,
  Created, LogInfo, Running, Task, TaskRun, TaskRunLog,
}
import pog

// =============================================================================
// Task Queries
// =============================================================================

// All task SELECT columns (used in multiple queries)
const task_select_cols = "id, project_id, title, description, acceptance_criteria, status,
            priority, model_name, dependencies, created_at, updated_at,
            started_at, completed_at, is_agentic, github_repo_url, queued_at, worker_id"

/// List all tasks, optionally filtered by project or status
pub fn list_tasks(
  db: Connection,
  project_id: Option(String),
  status_filter: Option(TaskStatus),
) -> Result(List(Task), String) {
  let base_sql = "SELECT " <> task_select_cols <> " FROM tasks"

  let #(sql, params) = case project_id, status_filter {
    None, None -> #(base_sql <> " ORDER BY priority ASC, updated_at DESC", [])
    Some(pid), None -> #(
      base_sql
        <> " WHERE project_id = $1 ORDER BY priority ASC, updated_at DESC",
      [pog.text(pid)],
    )
    None, Some(status) -> #(
      base_sql <> " WHERE status = $1 ORDER BY priority ASC, updated_at DESC",
      [pog.text(task.status_to_string(status))],
    )
    Some(pid), Some(status) -> #(
      base_sql
        <> " WHERE project_id = $1 AND status = $2 ORDER BY priority ASC, updated_at DESC",
      [pog.text(pid), pog.text(task.status_to_string(status))],
    )
  }

  execute_task_query(db, sql, params)
}

fn execute_task_query(
  db: Connection,
  sql: String,
  params: List(pog.Value),
) -> Result(List(Task), String) {
  let query = pog.query(sql)
  let query = list.fold(params, query, fn(q, p) { pog.parameter(q, p) })

  query
  |> pog.returning(task_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.rows })
  |> result.map_error(query_error_to_string)
}

/// Get a single task by ID
pub fn get_task(db: Connection, id: String) -> Result(Option(Task), String) {
  let sql = "SELECT " <> task_select_cols <> " FROM tasks WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.returning(task_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Create a new task
pub fn create_task(
  db: Connection,
  req: CreateTaskRequest,
) -> Result(Task, String) {
  let now = birl.to_iso8601(birl.now())
  let priority = option.unwrap(req.priority, 3)
  let deps = option.unwrap(req.dependencies, [])
  let deps_json = json.array(deps, json.string) |> json.to_string
  let is_agentic = option.unwrap(req.is_agentic, False)

  let sql =
    "INSERT INTO tasks (project_id, title, description, acceptance_criteria,
                        status, priority, model_name, dependencies,
                        is_agentic, github_repo_url, created_at, updated_at)
     VALUES ($1, $2, $3, $4, 'created', $5, $6, $7, $8, $9, $10, $11)
     RETURNING " <> task_select_cols

  pog.query(sql)
  |> pog.parameter(pog.text(req.project_id))
  |> pog.parameter(pog.text(req.title))
  |> pog.parameter(pog.text(req.description))
  |> pog.parameter(pog.nullable(pog.text, req.acceptance_criteria))
  |> pog.parameter(pog.int(priority))
  |> pog.parameter(pog.nullable(pog.text, req.model_name))
  |> pog.parameter(pog.text(deps_json))
  |> pog.parameter(pog.bool(is_agentic))
  |> pog.parameter(pog.nullable(pog.text, req.github_repo_url))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(now))
  |> pog.returning(task_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(t) -> t
      Error(_) -> panic as "Insert should return a row"
    }
  })
  |> result.map_error(query_error_to_string)
}

/// Update a task
pub fn update_task(
  db: Connection,
  id: String,
  req: UpdateTaskRequest,
) -> Result(Option(Task), String) {
  case get_task(db, id) {
    Ok(Some(existing)) -> {
      let now = birl.to_iso8601(birl.now())
      let title = option.unwrap(req.title, existing.title)
      let description = option.unwrap(req.description, existing.description)
      let acceptance_criteria = case req.acceptance_criteria {
        Some(ac) -> Some(ac)
        None -> existing.acceptance_criteria
      }
      let status = case req.status {
        Some(s) -> task.status_to_string(s)
        None -> task.status_to_string(existing.status)
      }
      let priority = option.unwrap(req.priority, existing.priority)
      let model_name = case req.model_name {
        Some(m) -> Some(m)
        None -> existing.model_name
      }
      let deps = option.unwrap(req.dependencies, existing.dependencies)
      let deps_json = json.array(deps, json.string) |> json.to_string
      let is_agentic = option.unwrap(req.is_agentic, existing.is_agentic)
      let github_repo_url = case req.github_repo_url {
        Some(url) -> Some(url)
        None -> existing.github_repo_url
      }

      let sql =
        "UPDATE tasks SET title = $1, description = $2, acceptance_criteria = $3,
                status = $4, priority = $5, model_name = $6, dependencies = $7,
                is_agentic = $8, github_repo_url = $9, updated_at = $10
         WHERE id = $11
         RETURNING "
        <> task_select_cols

      pog.query(sql)
      |> pog.parameter(pog.text(title))
      |> pog.parameter(pog.text(description))
      |> pog.parameter(pog.nullable(pog.text, acceptance_criteria))
      |> pog.parameter(pog.text(status))
      |> pog.parameter(pog.int(priority))
      |> pog.parameter(pog.nullable(pog.text, model_name))
      |> pog.parameter(pog.text(deps_json))
      |> pog.parameter(pog.bool(is_agentic))
      |> pog.parameter(pog.nullable(pog.text, github_repo_url))
      |> pog.parameter(pog.text(now))
      |> pog.parameter(pog.text(id))
      |> pog.returning(task_row_decoder())
      |> pog.execute(db)
      |> result.map(fn(returned) {
        list.first(returned.rows) |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// Update task status
pub fn update_task_status(
  db: Connection,
  id: String,
  status: TaskStatus,
) -> Result(Option(Task), String) {
  let now = birl.to_iso8601(birl.now())
  let status_str = task.status_to_string(status)

  // Also update started_at/completed_at based on status
  let sql = case status {
    task.InProgress ->
      "UPDATE tasks SET status = $1, started_at = COALESCE(started_at, $2), updated_at = $2
       WHERE id = $3 RETURNING "
      <> task_select_cols
    task.Complete ->
      "UPDATE tasks SET status = $1, completed_at = $2, updated_at = $2
       WHERE id = $3 RETURNING " <> task_select_cols
    task.Queued ->
      "UPDATE tasks SET status = $1, queued_at = COALESCE(queued_at, $2), updated_at = $2
       WHERE id = $3 RETURNING "
      <> task_select_cols
    _ -> "UPDATE tasks SET status = $1, updated_at = $2
       WHERE id = $3 RETURNING " <> task_select_cols
  }

  pog.query(sql)
  |> pog.parameter(pog.text(status_str))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(id))
  |> pog.returning(task_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Delete a task
pub fn delete_task(db: Connection, id: String) -> Result(Bool, String) {
  let sql = "DELETE FROM tasks WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.count > 0 })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Task Run Queries
// =============================================================================

/// Create a new task run
pub fn create_task_run(
  db: Connection,
  task_id: String,
) -> Result(TaskRun, String) {
  let now = birl.to_iso8601(birl.now())

  let sql =
    "INSERT INTO task_runs (task_id, status, progress_percent, started_at)
     VALUES ($1, 'running', 0, $2)
     RETURNING id, task_id, status, current_phase, progress_percent,
               started_at, completed_at, error_message"

  pog.query(sql)
  |> pog.parameter(pog.text(task_id))
  |> pog.parameter(pog.text(now))
  |> pog.returning(task_run_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(run) -> run
      Error(_) -> panic as "Insert should return a row"
    }
  })
  |> result.map_error(query_error_to_string)
}

/// Get a task run by ID
pub fn get_task_run(
  db: Connection,
  id: String,
) -> Result(Option(TaskRun), String) {
  let sql =
    "SELECT id, task_id, status, current_phase, progress_percent,
            started_at, completed_at, error_message
     FROM task_runs WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.returning(task_run_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// List runs for a task
pub fn list_task_runs(
  db: Connection,
  task_id: String,
) -> Result(List(TaskRun), String) {
  let sql =
    "SELECT id, task_id, status, current_phase, progress_percent,
            started_at, completed_at, error_message
     FROM task_runs WHERE task_id = $1
     ORDER BY started_at DESC"

  pog.query(sql)
  |> pog.parameter(pog.text(task_id))
  |> pog.returning(task_run_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.rows })
  |> result.map_error(query_error_to_string)
}

/// Update task run progress
pub fn update_run_progress(
  db: Connection,
  run_id: String,
  phase: String,
  progress_percent: Int,
) -> Result(Option(TaskRun), String) {
  let sql =
    "UPDATE task_runs SET current_phase = $1, progress_percent = $2
     WHERE id = $3
     RETURNING id, task_id, status, current_phase, progress_percent,
               started_at, completed_at, error_message"

  pog.query(sql)
  |> pog.parameter(pog.text(phase))
  |> pog.parameter(pog.int(progress_percent))
  |> pog.parameter(pog.text(run_id))
  |> pog.returning(task_run_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Complete a task run (success or failure)
pub fn complete_task_run(
  db: Connection,
  run_id: String,
  status: TaskRunStatus,
  error_message: Option(String),
) -> Result(Option(TaskRun), String) {
  let now = birl.to_iso8601(birl.now())
  let status_str = task.run_status_to_string(status)

  let sql =
    "UPDATE task_runs SET status = $1, completed_at = $2, error_message = $3,
            progress_percent = CASE WHEN $1 = 'completed' THEN 100 ELSE progress_percent END
     WHERE id = $4
     RETURNING id, task_id, status, current_phase, progress_percent,
               started_at, completed_at, error_message"

  pog.query(sql)
  |> pog.parameter(pog.text(status_str))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.nullable(pog.text, error_message))
  |> pog.parameter(pog.text(run_id))
  |> pog.returning(task_run_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Task Run Log Queries
// =============================================================================

/// Add a log entry to a task run
pub fn add_run_log(
  db: Connection,
  run_id: String,
  phase: String,
  agent_type: String,
  log_level: LogLevel,
  message: String,
) -> Result(TaskRunLog, String) {
  let now = birl.to_iso8601(birl.now())
  let level_str = task.log_level_to_string(log_level)

  let sql =
    "INSERT INTO task_run_logs (task_run_id, phase, agent_type, log_level, message, created_at)
     VALUES ($1, $2, $3, $4, $5, $6)
     RETURNING id, task_run_id, phase, agent_type, log_level, message, created_at"

  pog.query(sql)
  |> pog.parameter(pog.text(run_id))
  |> pog.parameter(pog.text(phase))
  |> pog.parameter(pog.text(agent_type))
  |> pog.parameter(pog.text(level_str))
  |> pog.parameter(pog.text(message))
  |> pog.parameter(pog.text(now))
  |> pog.returning(task_run_log_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(log) -> log
      Error(_) -> panic as "Insert should return a row"
    }
  })
  |> result.map_error(query_error_to_string)
}

/// List logs for a task run
pub fn list_run_logs(
  db: Connection,
  run_id: String,
) -> Result(List(TaskRunLog), String) {
  let sql =
    "SELECT id, task_run_id, phase, agent_type, log_level, message, created_at
     FROM task_run_logs WHERE task_run_id = $1
     ORDER BY created_at ASC"

  pog.query(sql)
  |> pog.parameter(pog.text(run_id))
  |> pog.returning(task_run_log_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.rows })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Decoders
// =============================================================================

fn task_row_decoder() -> decode.Decoder(Task) {
  use id <- decode.field(0, decode.string)
  use project_id <- decode.field(1, decode.string)
  use title <- decode.field(2, decode.string)
  use description <- decode.field(3, decode.string)
  use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
  use status_str <- decode.field(5, decode.string)
  use priority <- decode.field(6, decode.int)
  use model_name <- decode.field(7, decode.optional(decode.string))
  use dependencies_json <- decode.field(8, decode.string)
  use created_at <- decode.field(9, decode.string)
  use updated_at <- decode.field(10, decode.string)
  use started_at <- decode.field(11, decode.optional(decode.string))
  use completed_at <- decode.field(12, decode.optional(decode.string))
  use is_agentic <- decode.field(13, decode.bool)
  use github_repo_url <- decode.field(14, decode.optional(decode.string))
  use queued_at <- decode.field(15, decode.optional(decode.string))
  use worker_id <- decode.field(16, decode.optional(decode.string))

  let status = case task.status_from_string(status_str) {
    Ok(s) -> s
    Error(_) -> Created
  }

  let dependencies = parse_dependencies(dependencies_json)

  decode.success(Task(
    id: id,
    project_id: project_id,
    title: title,
    description: description,
    acceptance_criteria: acceptance_criteria,
    status: status,
    priority: priority,
    model_name: model_name,
    dependencies: dependencies,
    created_at: created_at,
    updated_at: updated_at,
    started_at: started_at,
    completed_at: completed_at,
    is_agentic: is_agentic,
    github_repo_url: github_repo_url,
    queued_at: queued_at,
    worker_id: worker_id,
  ))
}

fn parse_dependencies(json_str: String) -> List(String) {
  let decoder = decode.list(decode.string)
  case json.parse(json_str, decoder) {
    Ok(deps) -> deps
    Error(_) -> []
  }
}

fn task_run_row_decoder() -> decode.Decoder(TaskRun) {
  use id <- decode.field(0, decode.string)
  use task_id <- decode.field(1, decode.string)
  use status_str <- decode.field(2, decode.string)
  use current_phase <- decode.field(3, decode.optional(decode.string))
  use progress_percent <- decode.field(4, decode.int)
  use started_at <- decode.field(5, decode.string)
  use completed_at <- decode.field(6, decode.optional(decode.string))
  use error_message <- decode.field(7, decode.optional(decode.string))

  let status = case task.run_status_from_string(status_str) {
    Ok(s) -> s
    Error(_) -> Running
  }

  decode.success(TaskRun(
    id: id,
    task_id: task_id,
    status: status,
    current_phase: current_phase,
    progress_percent: progress_percent,
    started_at: started_at,
    completed_at: completed_at,
    error_message: error_message,
  ))
}

fn task_run_log_row_decoder() -> decode.Decoder(TaskRunLog) {
  use id <- decode.field(0, decode.string)
  use task_run_id <- decode.field(1, decode.string)
  use phase <- decode.field(2, decode.string)
  use agent_type <- decode.field(3, decode.string)
  use level_str <- decode.field(4, decode.string)
  use message <- decode.field(5, decode.string)
  use created_at <- decode.field(6, decode.string)

  let log_level = case task.log_level_from_string(level_str) {
    Ok(l) -> l
    Error(_) -> LogInfo
  }

  decode.success(TaskRunLog(
    id: id,
    task_run_id: task_run_id,
    phase: phase,
    agent_type: agent_type,
    log_level: log_level,
    message: message,
    created_at: created_at,
  ))
}

// =============================================================================
// Queue Management Functions
// =============================================================================

/// Add a task to the execution queue
pub fn enqueue_task(
  db: Connection,
  task_id: String,
  priority: Int,
) -> Result(Nil, String) {
  let sql =
    "INSERT INTO task_queue (task_id, priority)
     VALUES ($1, $2)
     ON CONFLICT (task_id) DO UPDATE SET priority = $2, queued_at = NOW()"

  pog.query(sql)
  |> pog.parameter(pog.text(task_id))
  |> pog.parameter(pog.int(priority))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Claim the next task from the queue for a worker
/// Returns the task_id and queue_id if successful
pub fn claim_next_task(
  db: Connection,
  worker_id: String,
) -> Result(Option(#(String, String)), String) {
  // Use the PostgreSQL function we created in the migration
  let sql = "SELECT * FROM claim_next_task($1)"

  pog.query(sql)
  |> pog.parameter(pog.text(worker_id))
  |> pog.returning({
    use task_id <- decode.field(0, decode.optional(decode.string))
    use queue_id <- decode.field(1, decode.optional(decode.string))
    decode.success(#(task_id, queue_id))
  })
  |> pog.execute(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(#(Some(tid), Some(qid))) -> Some(#(tid, qid))
      _ -> None
    }
  })
  |> result.map_error(query_error_to_string)
}

/// Release a task back to the queue (for graceful shutdown or failure)
pub fn release_task(
  db: Connection,
  task_id: String,
  error_message: Option(String),
) -> Result(Nil, String) {
  let sql = "SELECT release_task($1, $2)"

  pog.query(sql)
  |> pog.parameter(pog.text(task_id))
  |> pog.parameter(pog.nullable(pog.text, error_message))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Complete a task in the queue (removes from queue)
pub fn complete_task_in_queue(
  db: Connection,
  task_id: String,
  success: Bool,
) -> Result(Nil, String) {
  let sql = "SELECT complete_task_in_queue($1, $2)"

  pog.query(sql)
  |> pog.parameter(pog.text(task_id))
  |> pog.parameter(pog.bool(success))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Recover orphaned tasks (called on worker startup)
/// Returns the number of tasks recovered
pub fn recover_orphaned_tasks(db: Connection) -> Result(Int, String) {
  let sql = "SELECT recover_orphaned_tasks()"

  let count_decoder = {
    use count <- decode.field(0, decode.int)
    decode.success(count)
  }

  pog.query(sql)
  |> pog.returning(count_decoder)
  |> pog.execute(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(count) -> count
      Error(_) -> 0
    }
  })
  |> result.map_error(query_error_to_string)
}

/// Get queued tasks for a worker (for display/monitoring)
pub fn list_queued_tasks(db: Connection) -> Result(List(Task), String) {
  let sql = "SELECT " <> task_select_cols <> "
     FROM tasks t
     JOIN task_queue tq ON tq.task_id = t.id
     ORDER BY tq.priority DESC, tq.queued_at ASC"

  pog.query(sql)
  |> pog.returning(task_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.rows })
  |> result.map_error(query_error_to_string)
}

/// Update task worker assignment
pub fn assign_task_to_worker(
  db: Connection,
  task_id: String,
  worker_id: String,
) -> Result(Option(Task), String) {
  let now = birl.to_iso8601(birl.now())

  let sql = "UPDATE tasks SET worker_id = $1, status = 'in_progress',
            started_at = COALESCE(started_at, $2), updated_at = $2
     WHERE id = $3 RETURNING " <> task_select_cols

  pog.query(sql)
  |> pog.parameter(pog.text(worker_id))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(task_id))
  |> pog.returning(task_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Clear worker assignment from task
pub fn unassign_task_worker(
  db: Connection,
  task_id: String,
) -> Result(Option(Task), String) {
  let now = birl.to_iso8601(birl.now())

  let sql = "UPDATE tasks SET worker_id = NULL, updated_at = $1
     WHERE id = $2 RETURNING " <> task_select_cols

  pog.query(sql)
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(task_id))
  |> pog.returning(task_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}
