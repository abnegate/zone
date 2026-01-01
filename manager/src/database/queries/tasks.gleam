import birl
import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/dynamic/decode
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/time/timestamp
import models/task.{
  type CreateTaskRequest, type LogLevel, type Task, type TaskRun,
  type TaskRunLog, type TaskRunStatus, type TaskStatus, Created, LogInfo,
  Running, Task, TaskRun, TaskRunLog,
}
import youid/uuid

// =============================================================================
// Task Queries (using Squirrel-generated SQL)
// =============================================================================

/// List all tasks, optionally filtered by project or status
pub fn list_tasks(
  db: Connection,
  project_id: Option(String),
  status_filter: Option(TaskStatus),
) -> Result(List(Task), String) {
  case project_id, status_filter {
    None, None ->
      sql.list_tasks_all(db)
      |> result.map(fn(returned) { list.map(returned.rows, list_tasks_row_to_task) })
      |> result.map_error(query_error_to_string)

    Some(pid), None ->
      case uuid.from_string(pid) {
        Ok(uuid_id) ->
          sql.list_tasks_by_project(db, uuid_id)
          |> result.map(fn(returned) {
            list.map(returned.rows, list_tasks_by_project_row_to_task)
          })
          |> result.map_error(query_error_to_string)
        Error(_) -> Error("Invalid project UUID format")
      }

    None, Some(status) ->
      sql.list_tasks_by_status(db, task.status_to_string(status))
      |> result.map(fn(returned) {
        list.map(returned.rows, list_tasks_by_status_row_to_task)
      })
      |> result.map_error(query_error_to_string)

    Some(pid), Some(status) ->
      case uuid.from_string(pid) {
        Ok(uuid_id) ->
          sql.list_tasks_by_project_and_status(
            db,
            uuid_id,
            task.status_to_string(status),
          )
          |> result.map(fn(returned) {
            list.map(returned.rows, list_tasks_by_project_and_status_row_to_task)
          })
          |> result.map_error(query_error_to_string)
        Error(_) -> Error("Invalid project UUID format")
      }
  }
}

/// Get a single task by ID
pub fn get_task(db: Connection, id: String) -> Result(Option(Task), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) ->
      sql.get_task_by_id(db, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(get_task_row_to_task)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Create a new task
pub fn create_task(
  db: Connection,
  req: CreateTaskRequest,
) -> Result(Task, String) {
  case uuid.from_string(req.project_id) {
    Ok(project_uuid) -> {
      let now = timestamp.system_time()
      let priority = option.unwrap(req.priority, 3)
      let deps = option.unwrap(req.dependencies, [])
      let deps_json = json.array(deps, json.string)
      let is_agentic = option.unwrap(req.is_agentic, False)
      let acceptance_criteria = option.unwrap(req.acceptance_criteria, "")
      let model_name = option.unwrap(req.model_name, "")
      let github_repo_url = option.unwrap(req.github_repo_url, "")

      sql.create_task(
        db,
        project_uuid,
        req.title,
        req.description,
        acceptance_criteria,
        priority,
        model_name,
        deps_json,
        is_agentic,
        github_repo_url,
        now,
        now,
      )
      |> result.map(fn(returned) {
        case list.first(returned.rows) {
          Ok(row) -> create_task_row_to_task(row)
          Error(_) -> panic as "Insert should return a row"
        }
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid project UUID format")
  }
}

/// Update a task
pub fn update_task(
  db: Connection,
  id: String,
  req: task.UpdateTaskRequest,
) -> Result(Option(Task), String) {
  case get_task(db, id) {
    Ok(Some(existing)) -> {
      case uuid.from_string(id) {
        Ok(uuid_id) -> {
          let now = timestamp.system_time()
          let title = option.unwrap(req.title, existing.title)
          let description = option.unwrap(req.description, existing.description)
          let acceptance_criteria = case req.acceptance_criteria {
            Some(ac) -> ac
            None -> option.unwrap(existing.acceptance_criteria, "")
          }
          let status = case req.status {
            Some(s) -> task.status_to_string(s)
            None -> task.status_to_string(existing.status)
          }
          let priority = option.unwrap(req.priority, existing.priority)
          let model_name = case req.model_name {
            Some(m) -> m
            None -> option.unwrap(existing.model_name, "")
          }
          let deps = option.unwrap(req.dependencies, existing.dependencies)
          let deps_json = json.array(deps, json.string)
          let is_agentic = option.unwrap(req.is_agentic, existing.is_agentic)
          let github_repo_url = case req.github_repo_url {
            Some(url) -> url
            None -> option.unwrap(existing.github_repo_url, "")
          }

          sql.update_task(
            db,
            title,
            description,
            acceptance_criteria,
            status,
            priority,
            model_name,
            deps_json,
            is_agentic,
            github_repo_url,
            now,
            uuid_id,
          )
          |> result.map(fn(returned) {
            list.first(returned.rows)
            |> result.map(update_task_row_to_task)
            |> option.from_result
          })
          |> result.map_error(query_error_to_string)
        }
        Error(_) -> Error("Invalid UUID format")
      }
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
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      let status_str = task.status_to_string(status)

      // Use the appropriate status-specific SQL based on status
      case status {
        task.InProgress ->
          sql.update_task_status_in_progress(db, status_str, now, uuid_id)
          |> result.map(fn(returned) {
            list.first(returned.rows)
            |> result.map(update_task_status_in_progress_row_to_task)
            |> option.from_result
          })
          |> result.map_error(query_error_to_string)

        task.Complete ->
          sql.update_task_status_complete(db, status_str, now, uuid_id)
          |> result.map(fn(returned) {
            list.first(returned.rows)
            |> result.map(update_task_status_complete_row_to_task)
            |> option.from_result
          })
          |> result.map_error(query_error_to_string)

        task.Queued ->
          sql.update_task_status_queued(db, status_str, now, uuid_id)
          |> result.map(fn(returned) {
            list.first(returned.rows)
            |> result.map(update_task_status_queued_row_to_task)
            |> option.from_result
          })
          |> result.map_error(query_error_to_string)

        _ ->
          sql.update_task_status_generic(db, status_str, now, uuid_id)
          |> result.map(fn(returned) {
            list.first(returned.rows)
            |> result.map(update_task_status_generic_row_to_task)
            |> option.from_result
          })
          |> result.map_error(query_error_to_string)
      }
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Delete a task
pub fn delete_task(db: Connection, id: String) -> Result(Bool, String) {
  case uuid.from_string(id) {
    Ok(uuid_id) ->
      sql.delete_task(db, uuid_id)
      |> result.map(fn(returned) { returned.count > 0 })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

// =============================================================================
// Task Run Queries
// =============================================================================

/// Create a new task run
pub fn create_task_run(
  db: Connection,
  task_id: String,
) -> Result(TaskRun, String) {
  case uuid.from_string(task_id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()

      sql.create_task_run(db, uuid_id, now)
      |> result.map(fn(returned) {
        case list.first(returned.rows) {
          Ok(row) -> create_task_run_row_to_task_run(row)
          Error(_) -> panic as "Insert should return a row"
        }
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Get a task run by ID
pub fn get_task_run(
  db: Connection,
  id: String,
) -> Result(Option(TaskRun), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) ->
      sql.get_task_run_by_id(db, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(get_task_run_row_to_task_run)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// List runs for a task
pub fn list_task_runs(
  db: Connection,
  task_id: String,
) -> Result(List(TaskRun), String) {
  case uuid.from_string(task_id) {
    Ok(uuid_id) ->
      sql.list_task_runs(db, uuid_id)
      |> result.map(fn(returned) {
        list.map(returned.rows, list_task_runs_row_to_task_run)
      })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Update task run progress
pub fn update_run_progress(
  db: Connection,
  run_id: String,
  phase: String,
  progress_percent: Int,
) -> Result(Option(TaskRun), String) {
  case uuid.from_string(run_id) {
    Ok(uuid_id) ->
      sql.update_task_run_progress(db, phase, progress_percent, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(update_task_run_progress_row_to_task_run)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Complete a task run (success or failure)
pub fn complete_task_run(
  db: Connection,
  run_id: String,
  status: TaskRunStatus,
  error_message: Option(String),
) -> Result(Option(TaskRun), String) {
  case uuid.from_string(run_id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      let status_str = task.run_status_to_string(status)
      let error_msg = option.unwrap(error_message, "")

      sql.complete_task_run(db, status_str, now, error_msg, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(complete_task_run_row_to_task_run)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
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
  case uuid.from_string(run_id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      let level_str = task.log_level_to_string(log_level)

      sql.add_task_run_log(db, uuid_id, phase, agent_type, level_str, message, now)
      |> result.map(fn(returned) {
        case list.first(returned.rows) {
          Ok(row) -> add_task_run_log_row_to_task_run_log(row)
          Error(_) -> panic as "Insert should return a row"
        }
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// List logs for a task run
pub fn list_run_logs(
  db: Connection,
  run_id: String,
) -> Result(List(TaskRunLog), String) {
  case uuid.from_string(run_id) {
    Ok(uuid_id) ->
      sql.list_task_run_logs(db, uuid_id)
      |> result.map(fn(returned) {
        list.map(returned.rows, list_task_run_logs_row_to_task_run_log)
      })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
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
  case uuid.from_string(task_id) {
    Ok(uuid_id) ->
      sql.enqueue_task(db, uuid_id, priority)
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Claim the next task from the queue for a worker
/// Returns the task_id and queue_id if successful
pub fn claim_next_task(
  db: Connection,
  worker_id: String,
) -> Result(Option(#(String, String)), String) {
  sql.claim_next_task(db, worker_id)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(row) -> Some(#(uuid.to_string(row.task_id), uuid.to_string(row.queue_id)))
      Error(_) -> None
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
  case uuid.from_string(task_id) {
    Ok(uuid_id) -> {
      let error_msg = option.unwrap(error_message, "")
      sql.release_task(db, uuid_id, error_msg)
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Complete a task in the queue (removes from queue)
pub fn complete_task_in_queue(
  db: Connection,
  task_id: String,
  success: Bool,
) -> Result(Nil, String) {
  case uuid.from_string(task_id) {
    Ok(uuid_id) ->
      sql.complete_task_queue(db, uuid_id, success)
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Recover orphaned tasks (called on worker startup)
/// Returns the number of tasks recovered
pub fn recover_orphaned_tasks(db: Connection) -> Result(Int, String) {
  sql.recover_orphaned_tasks(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(row) -> row.recover_orphaned_tasks
      Error(_) -> 0
    }
  })
  |> result.map_error(query_error_to_string)
}

/// Get queued tasks for a worker (for display/monitoring)
pub fn list_queued_tasks(db: Connection) -> Result(List(Task), String) {
  sql.list_queued_tasks(db)
  |> result.map(fn(returned) {
    list.map(returned.rows, list_queued_tasks_row_to_task)
  })
  |> result.map_error(query_error_to_string)
}

/// Update task worker assignment
pub fn assign_task_to_worker(
  db: Connection,
  task_id: String,
  worker_id: String,
) -> Result(Option(Task), String) {
  case uuid.from_string(task_id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()

      sql.assign_task_worker(db, worker_id, now, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(assign_task_worker_row_to_task)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Clear worker assignment from task
pub fn unassign_task_worker(
  db: Connection,
  task_id: String,
) -> Result(Option(Task), String) {
  case uuid.from_string(task_id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()

      sql.unassign_task_worker(db, now, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(unassign_task_worker_row_to_task)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

// =============================================================================
// Row Mapping Helpers
// =============================================================================

fn timestamp_to_string(ts: Option(timestamp.Timestamp)) -> String {
  case ts {
    Some(t) -> {
      let #(seconds, _nanoseconds) = timestamp.to_unix_seconds_and_nanoseconds(t)
      birl.from_unix(seconds) |> birl.to_iso8601
    }
    None -> ""
  }
}

fn timestamp_to_option_string(
  ts: Option(timestamp.Timestamp),
) -> Option(String) {
  case ts {
    Some(t) -> {
      let #(seconds, _nanoseconds) = timestamp.to_unix_seconds_and_nanoseconds(t)
      Some(birl.from_unix(seconds) |> birl.to_iso8601)
    }
    None -> None
  }
}

fn parse_dependencies(json_str: Option(String)) -> List(String) {
  case json_str {
    Some(s) -> {
      case json.parse(s, decode.list(decode.string)) {
        Ok(deps) -> deps
        Error(_) -> []
      }
    }
    None -> []
  }
}

fn status_from_string(s: String) -> TaskStatus {
  case task.status_from_string(s) {
    Ok(status) -> status
    Error(_) -> Created
  }
}

fn run_status_from_string(s: String) -> TaskRunStatus {
  case task.run_status_from_string(s) {
    Ok(status) -> status
    Error(_) -> Running
  }
}

fn log_level_from_string(s: String) -> LogLevel {
  case task.log_level_from_string(s) {
    Ok(level) -> level
    Error(_) -> LogInfo
  }
}

// Task row mappers for each query type

fn list_tasks_row_to_task(row: sql.ListTasksAllRow) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn list_tasks_by_project_row_to_task(row: sql.ListTasksByProjectRow) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn list_tasks_by_status_row_to_task(row: sql.ListTasksByStatusRow) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn list_tasks_by_project_and_status_row_to_task(
  row: sql.ListTasksByProjectAndStatusRow,
) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn get_task_row_to_task(row: sql.GetTaskByIdRow) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn create_task_row_to_task(row: sql.CreateTaskRow) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn update_task_row_to_task(row: sql.UpdateTaskRow) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn update_task_status_in_progress_row_to_task(
  row: sql.UpdateTaskStatusInProgressRow,
) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn update_task_status_complete_row_to_task(
  row: sql.UpdateTaskStatusCompleteRow,
) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn update_task_status_queued_row_to_task(
  row: sql.UpdateTaskStatusQueuedRow,
) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn update_task_status_generic_row_to_task(
  row: sql.UpdateTaskStatusGenericRow,
) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn list_queued_tasks_row_to_task(row: sql.ListQueuedTasksRow) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn assign_task_worker_row_to_task(row: sql.AssignTaskWorkerRow) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

fn unassign_task_worker_row_to_task(row: sql.UnassignTaskWorkerRow) -> Task {
  Task(
    id: uuid.to_string(row.id),
    project_id: uuid.to_string(row.project_id),
    title: row.title,
    description: row.description,
    acceptance_criteria: row.acceptance_criteria,
    status: status_from_string(row.status),
    priority: option.unwrap(row.priority, 3),
    model_name: row.model_name,
    dependencies: parse_dependencies(row.dependencies),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
    started_at: timestamp_to_option_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    is_agentic: row.is_agentic,
    github_repo_url: row.github_repo_url,
    queued_at: timestamp_to_option_string(row.queued_at),
    worker_id: row.worker_id,
  )
}

// Task run row mappers

fn create_task_run_row_to_task_run(row: sql.CreateTaskRunRow) -> TaskRun {
  TaskRun(
    id: uuid.to_string(row.id),
    task_id: uuid.to_string(row.task_id),
    status: run_status_from_string(row.status),
    current_phase: row.current_phase,
    progress_percent: option.unwrap(row.progress_percent, 0),
    started_at: timestamp_to_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    error_message: row.error_message,
  )
}

fn get_task_run_row_to_task_run(row: sql.GetTaskRunByIdRow) -> TaskRun {
  TaskRun(
    id: uuid.to_string(row.id),
    task_id: uuid.to_string(row.task_id),
    status: run_status_from_string(row.status),
    current_phase: row.current_phase,
    progress_percent: option.unwrap(row.progress_percent, 0),
    started_at: timestamp_to_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    error_message: row.error_message,
  )
}

fn list_task_runs_row_to_task_run(row: sql.ListTaskRunsRow) -> TaskRun {
  TaskRun(
    id: uuid.to_string(row.id),
    task_id: uuid.to_string(row.task_id),
    status: run_status_from_string(row.status),
    current_phase: row.current_phase,
    progress_percent: option.unwrap(row.progress_percent, 0),
    started_at: timestamp_to_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    error_message: row.error_message,
  )
}

fn update_task_run_progress_row_to_task_run(
  row: sql.UpdateTaskRunProgressRow,
) -> TaskRun {
  TaskRun(
    id: uuid.to_string(row.id),
    task_id: uuid.to_string(row.task_id),
    status: run_status_from_string(row.status),
    current_phase: row.current_phase,
    progress_percent: option.unwrap(row.progress_percent, 0),
    started_at: timestamp_to_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    error_message: row.error_message,
  )
}

fn complete_task_run_row_to_task_run(row: sql.CompleteTaskRunRow) -> TaskRun {
  TaskRun(
    id: uuid.to_string(row.id),
    task_id: uuid.to_string(row.task_id),
    status: run_status_from_string(row.status),
    current_phase: row.current_phase,
    progress_percent: option.unwrap(row.progress_percent, 0),
    started_at: timestamp_to_string(row.started_at),
    completed_at: timestamp_to_option_string(row.completed_at),
    error_message: row.error_message,
  )
}

// Task run log row mappers

fn add_task_run_log_row_to_task_run_log(
  row: sql.AddTaskRunLogRow,
) -> TaskRunLog {
  TaskRunLog(
    id: uuid.to_string(row.id),
    task_run_id: uuid.to_string(row.task_run_id),
    phase: row.phase,
    agent_type: row.agent_type,
    log_level: log_level_from_string(row.log_level),
    message: row.message,
    created_at: timestamp_to_string(row.created_at),
  )
}

fn list_task_run_logs_row_to_task_run_log(
  row: sql.ListTaskRunLogsRow,
) -> TaskRunLog {
  TaskRunLog(
    id: uuid.to_string(row.id),
    task_run_id: uuid.to_string(row.task_run_id),
    phase: row.phase,
    agent_type: row.agent_type,
    log_level: log_level_from_string(row.log_level),
    message: row.message,
    created_at: timestamp_to_string(row.created_at),
  )
}
