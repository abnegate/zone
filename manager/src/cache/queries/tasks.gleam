/// Cached tasks queries - wraps database queries with Valkey caching
///
/// Note: Queue operations are NOT cached as they need to be real-time.
/// Task runs and logs are cached with short TTLs.
import cache/connection as cache
import config
import database/connection.{type Connection}
import database/queries/tasks as db_tasks
import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}
import models/task.{
  type CreateTaskRequest, type LogLevel, type Task, type TaskRun,
  type TaskRunLog, type TaskRunStatus, type TaskStatus, type UpdateTaskRequest,
  Task, TaskRun, TaskRunLog,
}

const entity_type = "task"

const run_entity_type = "task_run"

const log_entity_type = "task_log"

// =============================================================================
// Cached Task Queries
// =============================================================================

/// List all tasks with caching
pub fn list_tasks(
  db: Connection,
  cache_client: cache.CacheConnection,
  project_id: Option(String),
  status_filter: Option(TaskStatus),
) -> Result(List(Task), String) {
  let cache_key = case project_id, status_filter {
    None, None -> cache.list_key(entity_type)
    Some(pid), None -> cache.filtered_list_key(entity_type, "project:" <> pid)
    None, Some(status) ->
      cache.filtered_list_key(
        entity_type,
        "status:" <> task.status_to_string(status),
      )
    Some(pid), Some(status) ->
      cache.filtered_list_key(
        entity_type,
        "project:" <> pid <> ":status:" <> task.status_to_string(status),
      )
  }
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.list(task_decoder())) {
        Ok(tasks) -> Ok(tasks)
        Error(_) ->
          fetch_and_cache_tasks(
            db,
            cache_client,
            cache_key,
            ttl,
            project_id,
            status_filter,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_tasks(
        db,
        cache_client,
        cache_key,
        ttl,
        project_id,
        status_filter,
      )
    Error(_) -> db_tasks.list_tasks(db, project_id, status_filter)
  }
}

fn fetch_and_cache_tasks(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  project_id: Option(String),
  status_filter: Option(TaskStatus),
) -> Result(List(Task), String) {
  case db_tasks.list_tasks(db, project_id, status_filter) {
    Ok(tasks) -> {
      let json_str = json.to_string(json.array(tasks, task_to_json))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(tasks)
    }
    Error(err) -> Error(err)
  }
}

/// Get a single task by ID with caching
pub fn get_task(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Option(Task), String) {
  let cache_key = cache.entity_key(entity_type, id)
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, task_decoder()) {
        Ok(t) -> Ok(Some(t))
        Error(_) -> fetch_and_cache_task(db, cache_client, cache_key, ttl, id)
      }
    }
    Ok(None) -> fetch_and_cache_task(db, cache_client, cache_key, ttl, id)
    Error(_) -> db_tasks.get_task(db, id)
  }
}

fn fetch_and_cache_task(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  id: String,
) -> Result(Option(Task), String) {
  case db_tasks.get_task(db, id) {
    Ok(Some(t)) -> {
      let json_str = json.to_string(task_to_json(t))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(t))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// List task runs with caching (short TTL for active monitoring)
pub fn list_task_runs(
  db: Connection,
  cache_client: cache.CacheConnection,
  task_id: String,
) -> Result(List(TaskRun), String) {
  let cache_key = cache.entity_key(run_entity_type, "task:" <> task_id)
  let ttl = 30
  // Short TTL for active runs

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.list(task_run_decoder())) {
        Ok(runs) -> Ok(runs)
        Error(_) ->
          fetch_and_cache_runs(db, cache_client, cache_key, ttl, task_id)
      }
    }
    Ok(None) -> fetch_and_cache_runs(db, cache_client, cache_key, ttl, task_id)
    Error(_) -> db_tasks.list_task_runs(db, task_id)
  }
}

fn fetch_and_cache_runs(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  task_id: String,
) -> Result(List(TaskRun), String) {
  case db_tasks.list_task_runs(db, task_id) {
    Ok(runs) -> {
      let json_str = json.to_string(json.array(runs, task_run_to_json))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(runs)
    }
    Error(err) -> Error(err)
  }
}

/// Get a task run by ID with caching
pub fn get_task_run(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Option(TaskRun), String) {
  let cache_key = cache.entity_key(run_entity_type, id)
  let ttl = 30
  // Short TTL

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, task_run_decoder()) {
        Ok(run) -> Ok(Some(run))
        Error(_) -> fetch_and_cache_run(db, cache_client, cache_key, ttl, id)
      }
    }
    Ok(None) -> fetch_and_cache_run(db, cache_client, cache_key, ttl, id)
    Error(_) -> db_tasks.get_task_run(db, id)
  }
}

fn fetch_and_cache_run(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  id: String,
) -> Result(Option(TaskRun), String) {
  case db_tasks.get_task_run(db, id) {
    Ok(Some(run)) -> {
      let json_str = json.to_string(task_run_to_json(run))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(run))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// List run logs with caching
pub fn list_run_logs(
  db: Connection,
  cache_client: cache.CacheConnection,
  run_id: String,
) -> Result(List(TaskRunLog), String) {
  let cache_key = cache.entity_key(log_entity_type, "run:" <> run_id)
  let ttl = 60
  // Short TTL for logs

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.list(task_run_log_decoder())) {
        Ok(logs) -> Ok(logs)
        Error(_) ->
          fetch_and_cache_logs(db, cache_client, cache_key, ttl, run_id)
      }
    }
    Ok(None) -> fetch_and_cache_logs(db, cache_client, cache_key, ttl, run_id)
    Error(_) -> db_tasks.list_run_logs(db, run_id)
  }
}

fn fetch_and_cache_logs(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  run_id: String,
) -> Result(List(TaskRunLog), String) {
  case db_tasks.list_run_logs(db, run_id) {
    Ok(logs) -> {
      let json_str = json.to_string(json.array(logs, task_run_log_to_json))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(logs)
    }
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Write Operations (with cache invalidation)
// =============================================================================

/// Create a new task
pub fn create_task(
  db: Connection,
  cache_client: cache.CacheConnection,
  req: CreateTaskRequest,
) -> Result(Task, String) {
  case db_tasks.create_task(db, req) {
    Ok(t) -> {
      invalidate_task_cache(cache_client)
      Ok(t)
    }
    Error(err) -> Error(err)
  }
}

/// Update a task
pub fn update_task(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
  req: UpdateTaskRequest,
) -> Result(Option(Task), String) {
  case db_tasks.update_task(db, id, req) {
    Ok(result) -> {
      invalidate_task_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Update task status
pub fn update_task_status(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
  status: TaskStatus,
) -> Result(Option(Task), String) {
  case db_tasks.update_task_status(db, id, status) {
    Ok(result) -> {
      invalidate_task_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Delete a task
pub fn delete_task(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Bool, String) {
  case db_tasks.delete_task(db, id) {
    Ok(result) -> {
      invalidate_task_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Create a task run
pub fn create_task_run(
  db: Connection,
  cache_client: cache.CacheConnection,
  task_id: String,
) -> Result(TaskRun, String) {
  case db_tasks.create_task_run(db, task_id) {
    Ok(run) -> {
      invalidate_runs_cache(cache_client, task_id)
      Ok(run)
    }
    Error(err) -> Error(err)
  }
}

/// Update run progress
pub fn update_run_progress(
  db: Connection,
  cache_client: cache.CacheConnection,
  run_id: String,
  phase: String,
  progress_percent: Int,
) -> Result(Option(TaskRun), String) {
  case db_tasks.update_run_progress(db, run_id, phase, progress_percent) {
    Ok(result) -> {
      let _ =
        cache.delete(cache_client, cache.entity_key(run_entity_type, run_id))
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Complete a task run
pub fn complete_task_run(
  db: Connection,
  cache_client: cache.CacheConnection,
  run_id: String,
  status: TaskRunStatus,
  error_message: Option(String),
) -> Result(Option(TaskRun), String) {
  case db_tasks.complete_task_run(db, run_id, status, error_message) {
    Ok(result) -> {
      let _ =
        cache.delete(cache_client, cache.entity_key(run_entity_type, run_id))
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Add a run log
pub fn add_run_log(
  db: Connection,
  cache_client: cache.CacheConnection,
  run_id: String,
  phase: String,
  agent_type: String,
  log_level: LogLevel,
  message: String,
) -> Result(TaskRunLog, String) {
  case db_tasks.add_run_log(db, run_id, phase, agent_type, log_level, message) {
    Ok(log) -> {
      let _ =
        cache.delete(
          cache_client,
          cache.entity_key(log_entity_type, "run:" <> run_id),
        )
      Ok(log)
    }
    Error(err) -> Error(err)
  }
}

/// Assign task to worker
pub fn assign_task_to_worker(
  db: Connection,
  cache_client: cache.CacheConnection,
  task_id: String,
  worker_id: String,
) -> Result(Option(Task), String) {
  case db_tasks.assign_task_to_worker(db, task_id, worker_id) {
    Ok(result) -> {
      invalidate_task_by_id(cache_client, task_id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Unassign task worker
pub fn unassign_task_worker(
  db: Connection,
  cache_client: cache.CacheConnection,
  task_id: String,
) -> Result(Option(Task), String) {
  case db_tasks.unassign_task_worker(db, task_id) {
    Ok(result) -> {
      invalidate_task_by_id(cache_client, task_id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Queue Operations (NOT cached - need real-time data)
// =============================================================================

pub fn enqueue_task(
  db: Connection,
  task_id: String,
  priority: Int,
) -> Result(Nil, String) {
  db_tasks.enqueue_task(db, task_id, priority)
}

pub fn claim_next_task(
  db: Connection,
  worker_id: String,
) -> Result(Option(#(String, String)), String) {
  db_tasks.claim_next_task(db, worker_id)
}

pub fn release_task(
  db: Connection,
  task_id: String,
  error_message: Option(String),
) -> Result(Nil, String) {
  db_tasks.release_task(db, task_id, error_message)
}

pub fn complete_task_in_queue(
  db: Connection,
  task_id: String,
  success: Bool,
) -> Result(Nil, String) {
  db_tasks.complete_task_in_queue(db, task_id, success)
}

pub fn recover_orphaned_tasks(db: Connection) -> Result(Int, String) {
  db_tasks.recover_orphaned_tasks(db)
}

pub fn list_queued_tasks(db: Connection) -> Result(List(Task), String) {
  // Queue list is not cached - needs real-time data
  db_tasks.list_queued_tasks(db)
}

// =============================================================================
// Cache Invalidation
// =============================================================================

fn invalidate_task_cache(cache_client: cache.CacheConnection) -> Nil {
  let _ =
    cache.delete_pattern(cache_client, cache.invalidation_pattern(entity_type))
  Nil
}

fn invalidate_task_by_id(cache_client: cache.CacheConnection, id: String) -> Nil {
  let _ = cache.delete(cache_client, cache.entity_key(entity_type, id))
  invalidate_task_cache(cache_client)
}

fn invalidate_runs_cache(
  cache_client: cache.CacheConnection,
  task_id: String,
) -> Nil {
  let _ =
    cache.delete(
      cache_client,
      cache.entity_key(run_entity_type, "task:" <> task_id),
    )
  Nil
}

// =============================================================================
// JSON Serialization
// =============================================================================

fn task_to_json(t: Task) -> json.Json {
  json.object([
    #("id", json.string(t.id)),
    #("project_id", json.string(t.project_id)),
    #("title", json.string(t.title)),
    #("description", json.string(t.description)),
    #("acceptance_criteria", json.nullable(t.acceptance_criteria, json.string)),
    #("status", json.string(task.status_to_string(t.status))),
    #("priority", json.int(t.priority)),
    #("model_name", json.nullable(t.model_name, json.string)),
    #("dependencies", json.array(t.dependencies, json.string)),
    #("created_at", json.string(t.created_at)),
    #("updated_at", json.string(t.updated_at)),
    #("started_at", json.nullable(t.started_at, json.string)),
    #("completed_at", json.nullable(t.completed_at, json.string)),
    #("is_agentic", json.bool(t.is_agentic)),
    #("github_repo_url", json.nullable(t.github_repo_url, json.string)),
    #("queued_at", json.nullable(t.queued_at, json.string)),
    #("worker_id", json.nullable(t.worker_id, json.string)),
  ])
}

fn task_decoder() -> decode.Decoder(Task) {
  use id <- decode.field("id", decode.string)
  use project_id <- decode.field("project_id", decode.string)
  use title <- decode.field("title", decode.string)
  use description <- decode.field("description", decode.string)
  use acceptance_criteria <- decode.field(
    "acceptance_criteria",
    decode.optional(decode.string),
  )
  use status_str <- decode.field("status", decode.string)
  use priority <- decode.field("priority", decode.int)
  use model_name <- decode.field("model_name", decode.optional(decode.string))
  use dependencies <- decode.field("dependencies", decode.list(decode.string))
  use created_at <- decode.field("created_at", decode.string)
  use updated_at <- decode.field("updated_at", decode.string)
  use started_at <- decode.field("started_at", decode.optional(decode.string))
  use completed_at <- decode.field(
    "completed_at",
    decode.optional(decode.string),
  )
  use is_agentic <- decode.field("is_agentic", decode.bool)
  use github_repo_url <- decode.field(
    "github_repo_url",
    decode.optional(decode.string),
  )
  use queued_at <- decode.field("queued_at", decode.optional(decode.string))
  use worker_id <- decode.field("worker_id", decode.optional(decode.string))
  let status = case task.status_from_string(status_str) {
    Ok(s) -> s
    Error(_) -> task.Created
  }
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

fn task_run_to_json(r: TaskRun) -> json.Json {
  json.object([
    #("id", json.string(r.id)),
    #("task_id", json.string(r.task_id)),
    #("status", json.string(task.run_status_to_string(r.status))),
    #("current_phase", json.nullable(r.current_phase, json.string)),
    #("progress_percent", json.int(r.progress_percent)),
    #("started_at", json.string(r.started_at)),
    #("completed_at", json.nullable(r.completed_at, json.string)),
    #("error_message", json.nullable(r.error_message, json.string)),
  ])
}

fn task_run_decoder() -> decode.Decoder(TaskRun) {
  use id <- decode.field("id", decode.string)
  use task_id <- decode.field("task_id", decode.string)
  use status_str <- decode.field("status", decode.string)
  use current_phase <- decode.field(
    "current_phase",
    decode.optional(decode.string),
  )
  use progress_percent <- decode.field("progress_percent", decode.int)
  use started_at <- decode.field("started_at", decode.string)
  use completed_at <- decode.field(
    "completed_at",
    decode.optional(decode.string),
  )
  use error_message <- decode.field(
    "error_message",
    decode.optional(decode.string),
  )
  let status = case task.run_status_from_string(status_str) {
    Ok(s) -> s
    Error(_) -> task.Running
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

fn task_run_log_to_json(l: TaskRunLog) -> json.Json {
  json.object([
    #("id", json.string(l.id)),
    #("task_run_id", json.string(l.task_run_id)),
    #("phase", json.string(l.phase)),
    #("agent_type", json.string(l.agent_type)),
    #("log_level", json.string(task.log_level_to_string(l.log_level))),
    #("message", json.string(l.message)),
    #("created_at", json.string(l.created_at)),
  ])
}

fn task_run_log_decoder() -> decode.Decoder(TaskRunLog) {
  use id <- decode.field("id", decode.string)
  use task_run_id <- decode.field("task_run_id", decode.string)
  use phase <- decode.field("phase", decode.string)
  use agent_type <- decode.field("agent_type", decode.string)
  use level_str <- decode.field("log_level", decode.string)
  use message <- decode.field("message", decode.string)
  use created_at <- decode.field("created_at", decode.string)
  let log_level = case task.log_level_from_string(level_str) {
    Ok(l) -> l
    Error(_) -> task.LogInfo
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
