//// This module contains the code to run the sql queries defined in
//// `./src/database/queries/sql`.
//// > 🐿️ This module was generated automatically using v4.6.0 of
//// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
////

import gleam/dynamic/decode
import gleam/json.{type Json}
import gleam/option.{type Option}
import gleam/time/timestamp.{type Timestamp}
import pog
import youid/uuid.{type Uuid}

/// A row you get from running the `add_task_run_log` query
/// defined in `./src/database/queries/sql/add_task_run_log.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type AddTaskRunLogRow {
  AddTaskRunLogRow(
    id: Uuid,
    task_run_id: Uuid,
    phase: String,
    agent_type: String,
    log_level: String,
    message: String,
    created_at: Option(Timestamp),
  )
}

/// Add a log entry to a task run
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn add_task_run_log(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: String,
  arg_3: String,
  arg_4: String,
  arg_5: String,
  arg_6: Timestamp,
) -> Result(pog.Returned(AddTaskRunLogRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use task_run_id <- decode.field(1, uuid_decoder())
    use phase <- decode.field(2, decode.string)
    use agent_type <- decode.field(3, decode.string)
    use log_level <- decode.field(4, decode.string)
    use message <- decode.field(5, decode.string)
    use created_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(AddTaskRunLogRow(
      id:,
      task_run_id:,
      phase:,
      agent_type:,
      log_level:,
      message:,
      created_at:,
    ))
  }

  "-- Add a log entry to a task run
INSERT INTO task_run_logs (task_run_id, phase, agent_type, log_level, message, created_at)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING id, task_run_id, phase, agent_type, log_level, message, created_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.text(arg_4))
  |> pog.parameter(pog.text(arg_5))
  |> pog.parameter(pog.timestamp(arg_6))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `archive_chat` query
/// defined in `./src/database/queries/sql/archive_chat.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ArchiveChatRow {
  ArchiveChatRow(
    id: Uuid,
    title: String,
    model_name: String,
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    archived: Option(Bool),
  )
}

/// Archive a chat
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn archive_chat(
  db: pog.Connection,
  arg_1: Timestamp,
  arg_2: Uuid,
) -> Result(pog.Returned(ArchiveChatRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use title <- decode.field(1, decode.string)
    use model_name <- decode.field(2, decode.string)
    use created_at <- decode.field(3, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    use archived <- decode.field(5, decode.optional(decode.bool))
    decode.success(ArchiveChatRow(
      id:,
      title:,
      model_name:,
      created_at:,
      updated_at:,
      archived:,
    ))
  }

  "-- Archive a chat
UPDATE chats
SET archived = true, updated_at = $1
WHERE id = $2
RETURNING id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
"
  |> pog.query
  |> pog.parameter(pog.timestamp(arg_1))
  |> pog.parameter(pog.text(uuid.to_string(arg_2)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `assign_task_worker` query
/// defined in `./src/database/queries/sql/assign_task_worker.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type AssignTaskWorkerRow {
  AssignTaskWorkerRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// Assign task to worker
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn assign_task_worker(
  db: pog.Connection,
  arg_1: String,
  arg_2: Timestamp,
  arg_3: Uuid,
) -> Result(pog.Returned(AssignTaskWorkerRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(AssignTaskWorkerRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- Assign task to worker
UPDATE tasks
SET worker_id = $1, status = 'in_progress',
    started_at = COALESCE(started_at::timestamp, $2), updated_at = $2
WHERE id = $3
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Assign a role to a user
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn assign_user_role(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: Uuid,
  arg_3: String,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Assign a role to a user
INSERT INTO user_roles (user_id, role_id, assigned_by)
SELECT $1, r.id, $2
FROM roles r
WHERE r.name = $3
ON CONFLICT DO NOTHING
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(uuid.to_string(arg_2)))
  |> pog.parameter(pog.text(arg_3))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `claim_next_task` query
/// defined in `./src/database/queries/sql/claim_next_task.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ClaimNextTaskRow {
  ClaimNextTaskRow(task_id: Uuid, queue_id: Uuid)
}

/// Claim the next task from the queue for a worker
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn claim_next_task(
  db: pog.Connection,
  arg_1: String,
) -> Result(pog.Returned(ClaimNextTaskRow), pog.QueryError) {
  let decoder = {
    use task_id <- decode.field(0, uuid_decoder())
    use queue_id <- decode.field(1, uuid_decoder())
    decode.success(ClaimNextTaskRow(task_id:, queue_id:))
  }

  "-- Claim the next task from the queue for a worker
SELECT * FROM claim_next_task($1)
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Clean up expired tokens
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn cleanup_expired_tokens(
  db: pog.Connection,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Clean up expired tokens
DELETE FROM refresh_tokens
WHERE expires_at < NOW()
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `complete_task_queue` query
/// defined in `./src/database/queries/sql/complete_task_queue.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CompleteTaskQueueRow {
  CompleteTaskQueueRow(success: Bool)
}

/// Complete a task in the queue (removes from queue)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn complete_task_queue(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: Bool,
) -> Result(pog.Returned(CompleteTaskQueueRow), pog.QueryError) {
  let decoder = {
    use success <- decode.field(0, decode.bool)
    decode.success(CompleteTaskQueueRow(success:))
  }

  "-- Complete a task in the queue (removes from queue)
SELECT (complete_task_in_queue($1, $2) IS NULL) AS success
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.bool(arg_2))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `complete_task_run` query
/// defined in `./src/database/queries/sql/complete_task_run.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CompleteTaskRunRow {
  CompleteTaskRunRow(
    id: Uuid,
    task_id: Uuid,
    status: String,
    current_phase: Option(String),
    progress_percent: Option(Int),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    error_message: Option(String),
  )
}

/// Complete a task run (success or failure)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn complete_task_run(
  db: pog.Connection,
  arg_1: String,
  arg_2: Timestamp,
  arg_3: String,
  arg_4: Uuid,
) -> Result(pog.Returned(CompleteTaskRunRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use task_id <- decode.field(1, uuid_decoder())
    use status <- decode.field(2, decode.string)
    use current_phase <- decode.field(3, decode.optional(decode.string))
    use progress_percent <- decode.field(4, decode.optional(decode.int))
    use started_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      6,
      decode.optional(pog.timestamp_decoder()),
    )
    use error_message <- decode.field(7, decode.optional(decode.string))
    decode.success(CompleteTaskRunRow(
      id:,
      task_id:,
      status:,
      current_phase:,
      progress_percent:,
      started_at:,
      completed_at:,
      error_message:,
    ))
  }

  "-- Complete a task run (success or failure)
UPDATE task_runs
SET status = $1, completed_at = $2, error_message = $3,
    progress_percent = CASE WHEN $1 = 'completed' THEN 100 ELSE progress_percent END
WHERE id = $4
RETURNING id, task_id, status, current_phase, progress_percent,
          started_at::timestamp, completed_at::timestamp, error_message
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.text(uuid.to_string(arg_4)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `count_user_tokens` query
/// defined in `./src/database/queries/sql/count_user_tokens.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CountUserTokensRow {
  CountUserTokensRow(count: Int)
}

/// Get count of active tokens for a user
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn count_user_tokens(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(CountUserTokensRow), pog.QueryError) {
  let decoder = {
    use count <- decode.field(0, decode.int)
    decode.success(CountUserTokensRow(count:))
  }

  "-- Get count of active tokens for a user
SELECT COUNT(*)::int
FROM refresh_tokens
WHERE user_id = $1 AND expires_at > NOW() AND revoked_at IS NULL
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `count_users` query
/// defined in `./src/database/queries/sql/count_users.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CountUsersRow {
  CountUsersRow(count: Int)
}

/// Count all users (for first-user-is-admin logic)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn count_users(
  db: pog.Connection,
) -> Result(pog.Returned(CountUsersRow), pog.QueryError) {
  let decoder = {
    use count <- decode.field(0, decode.int)
    decode.success(CountUsersRow(count:))
  }

  "-- Count all users (for first-user-is-admin logic)
SELECT COUNT(*)::int FROM users
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `create_chat` query
/// defined in `./src/database/queries/sql/create_chat.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CreateChatRow {
  CreateChatRow(
    id: Uuid,
    title: String,
    model_name: String,
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    archived: Option(Bool),
  )
}

/// Create a new chat
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn create_chat(
  db: pog.Connection,
  arg_1: String,
  arg_2: String,
  arg_3: Timestamp,
  arg_4: Timestamp,
) -> Result(pog.Returned(CreateChatRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use title <- decode.field(1, decode.string)
    use model_name <- decode.field(2, decode.string)
    use created_at <- decode.field(3, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    use archived <- decode.field(5, decode.optional(decode.bool))
    decode.success(CreateChatRow(
      id:,
      title:,
      model_name:,
      created_at:,
      updated_at:,
      archived:,
    ))
  }

  "-- Create a new chat
INSERT INTO chats (title, model_name, created_at, updated_at, archived)
VALUES ($1, $2, $3, $4, false)
RETURNING id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.timestamp(arg_3))
  |> pog.parameter(pog.timestamp(arg_4))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `create_message` query
/// defined in `./src/database/queries/sql/create_message.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CreateMessageRow {
  CreateMessageRow(
    id: Uuid,
    chat_id: Uuid,
    role: String,
    content: String,
    created_at: Option(Timestamp),
  )
}

/// Create a new message
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn create_message(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: String,
  arg_3: String,
  arg_4: Timestamp,
) -> Result(pog.Returned(CreateMessageRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use chat_id <- decode.field(1, uuid_decoder())
    use role <- decode.field(2, decode.string)
    use content <- decode.field(3, decode.string)
    use created_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    decode.success(CreateMessageRow(id:, chat_id:, role:, content:, created_at:))
  }

  "-- Create a new message
INSERT INTO messages (chat_id, role, content, created_at)
VALUES ($1, $2, $3, $4)
RETURNING id, chat_id, role, content, created_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.timestamp(arg_4))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `create_organization` query
/// defined in `./src/database/queries/sql/create_organization.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CreateOrganizationRow {
  CreateOrganizationRow(
    id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Create a new organization
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn create_organization(
  db: pog.Connection,
  arg_1: String,
  arg_2: String,
  arg_3: String,
  arg_4: Timestamp,
  arg_5: Timestamp,
) -> Result(pog.Returned(CreateOrganizationRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use slug <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.optional(decode.string))
    use is_active <- decode.field(4, decode.optional(decode.bool))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(CreateOrganizationRow(
      id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Create a new organization
INSERT INTO organizations (name, slug, description, created_at, updated_at)
VALUES ($1, $2, $3, $4, $5)
RETURNING id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.timestamp(arg_4))
  |> pog.parameter(pog.timestamp(arg_5))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `create_project` query
/// defined in `./src/database/queries/sql/create_project.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CreateProjectRow {
  CreateProjectRow(
    id: Uuid,
    name: String,
    description: Option(String),
    status: String,
    github_repo_url: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Create a new project
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn create_project(
  db: pog.Connection,
  arg_1: String,
  arg_2: String,
  arg_3: String,
  arg_4: String,
  arg_5: Timestamp,
  arg_6: Timestamp,
) -> Result(pog.Returned(CreateProjectRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use description <- decode.field(2, decode.optional(decode.string))
    use status <- decode.field(3, decode.string)
    use github_repo_url <- decode.field(4, decode.optional(decode.string))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(CreateProjectRow(
      id:,
      name:,
      description:,
      status:,
      github_repo_url:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Create a new project
INSERT INTO projects (name, description, status, github_repo_url, created_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING id, name, description, status, github_repo_url, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.text(arg_4))
  |> pog.parameter(pog.timestamp(arg_5))
  |> pog.parameter(pog.timestamp(arg_6))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Store a refresh token (hashed)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn create_refresh_token(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: String,
  arg_3: Timestamp,
  arg_4: String,
  arg_5: String,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Store a refresh token (hashed)
INSERT INTO refresh_tokens (user_id, token_hash, expires_at, user_agent, ip_address)
VALUES ($1, $2, $3::timestamp, $4, $5)
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.timestamp(arg_3))
  |> pog.parameter(pog.text(arg_4))
  |> pog.parameter(pog.text(arg_5))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `create_source` query
/// defined in `./src/database/queries/sql/create_source.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CreateSourceRow {
  CreateSourceRow(
    id: Uuid,
    name: String,
    source_type: String,
    config: String,
    credentials_encrypted: Option(String),
    description: Option(String),
    url: Option(String),
    is_active: Option(Bool),
    last_verified_at: Option(Timestamp),
    last_error: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Create a new source
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn create_source(
  db: pog.Connection,
  arg_1: String,
  arg_2: String,
  arg_3: Json,
  arg_4: String,
  arg_5: String,
  arg_6: String,
  arg_7: Timestamp,
  arg_8: Timestamp,
) -> Result(pog.Returned(CreateSourceRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use source_type <- decode.field(2, decode.string)
    use config <- decode.field(3, decode.string)
    use credentials_encrypted <- decode.field(4, decode.optional(decode.string))
    use description <- decode.field(5, decode.optional(decode.string))
    use url <- decode.field(6, decode.optional(decode.string))
    use is_active <- decode.field(7, decode.optional(decode.bool))
    use last_verified_at <- decode.field(
      8,
      decode.optional(pog.timestamp_decoder()),
    )
    use last_error <- decode.field(9, decode.optional(decode.string))
    use created_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    decode.success(CreateSourceRow(
      id:,
      name:,
      source_type:,
      config:,
      credentials_encrypted:,
      description:,
      url:,
      is_active:,
      last_verified_at:,
      last_error:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Create a new source
INSERT INTO sources (name, source_type, config, credentials_encrypted, description, url, created_at, updated_at)
VALUES ($1, $2, $3::jsonb, $4, $5, $6, $7, $8)
RETURNING id, name, source_type, config, credentials_encrypted, description, url,
          is_active, last_verified_at::timestamp, last_error, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(json.to_string(arg_3)))
  |> pog.parameter(pog.text(arg_4))
  |> pog.parameter(pog.text(arg_5))
  |> pog.parameter(pog.text(arg_6))
  |> pog.parameter(pog.timestamp(arg_7))
  |> pog.parameter(pog.timestamp(arg_8))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `create_task` query
/// defined in `./src/database/queries/sql/create_task.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CreateTaskRow {
  CreateTaskRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// Create a new task
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn create_task(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: String,
  arg_3: String,
  arg_4: String,
  arg_5: Int,
  arg_6: String,
  arg_7: Json,
  arg_8: Bool,
  arg_9: String,
  arg_10: Timestamp,
  arg_11: Timestamp,
) -> Result(pog.Returned(CreateTaskRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(CreateTaskRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- Create a new task
INSERT INTO tasks (project_id, title, description, acceptance_criteria,
                   status, priority, model_name, dependencies,
                   is_agentic, github_repo_url, created_at, updated_at)
VALUES ($1, $2, $3, $4, 'created', $5, $6, $7, $8, $9, $10, $11)
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.text(arg_4))
  |> pog.parameter(pog.int(arg_5))
  |> pog.parameter(pog.text(arg_6))
  |> pog.parameter(pog.text(json.to_string(arg_7)))
  |> pog.parameter(pog.bool(arg_8))
  |> pog.parameter(pog.text(arg_9))
  |> pog.parameter(pog.timestamp(arg_10))
  |> pog.parameter(pog.timestamp(arg_11))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `create_task_run` query
/// defined in `./src/database/queries/sql/create_task_run.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CreateTaskRunRow {
  CreateTaskRunRow(
    id: Uuid,
    task_id: Uuid,
    status: String,
    current_phase: Option(String),
    progress_percent: Option(Int),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    error_message: Option(String),
  )
}

/// Create a new task run
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn create_task_run(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: Timestamp,
) -> Result(pog.Returned(CreateTaskRunRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use task_id <- decode.field(1, uuid_decoder())
    use status <- decode.field(2, decode.string)
    use current_phase <- decode.field(3, decode.optional(decode.string))
    use progress_percent <- decode.field(4, decode.optional(decode.int))
    use started_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      6,
      decode.optional(pog.timestamp_decoder()),
    )
    use error_message <- decode.field(7, decode.optional(decode.string))
    decode.success(CreateTaskRunRow(
      id:,
      task_id:,
      status:,
      current_phase:,
      progress_percent:,
      started_at:,
      completed_at:,
      error_message:,
    ))
  }

  "-- Create a new task run
INSERT INTO task_runs (task_id, status, progress_percent, started_at)
VALUES ($1, 'running', 0, $2)
RETURNING id, task_id, status, current_phase, progress_percent,
          started_at::timestamp, completed_at::timestamp, error_message
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `create_user` query
/// defined in `./src/database/queries/sql/create_user.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CreateUserRow {
  CreateUserRow(
    id: Uuid,
    email: String,
    display_name: Option(String),
    is_active: Option(Bool),
    is_admin: Option(Bool),
    created_at: String,
    updated_at: String,
    last_login_at: String,
  )
}

/// Create a new user
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn create_user(
  db: pog.Connection,
  arg_1: String,
  arg_2: String,
  arg_3: String,
  arg_4: Bool,
) -> Result(pog.Returned(CreateUserRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use email <- decode.field(1, decode.string)
    use display_name <- decode.field(2, decode.optional(decode.string))
    use is_active <- decode.field(3, decode.optional(decode.bool))
    use is_admin <- decode.field(4, decode.optional(decode.bool))
    use created_at <- decode.field(5, decode.string)
    use updated_at <- decode.field(6, decode.string)
    use last_login_at <- decode.field(7, decode.string)
    decode.success(CreateUserRow(
      id:,
      email:,
      display_name:,
      is_active:,
      is_admin:,
      created_at:,
      updated_at:,
      last_login_at:,
    ))
  }

  "-- Create a new user
INSERT INTO users (email, password_hash, display_name, is_admin)
VALUES ($1, $2, $3, $4)
RETURNING id, email, display_name, is_active, is_admin,
          created_at::text, updated_at::text, COALESCE(last_login_at::text, '') AS last_login_at
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.bool(arg_4))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `create_workspace` query
/// defined in `./src/database/queries/sql/create_workspace.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type CreateWorkspaceRow {
  CreateWorkspaceRow(
    id: Uuid,
    organization_id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Create a new workspace within an organization
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn create_workspace(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: String,
  arg_3: String,
  arg_4: String,
  arg_5: Timestamp,
  arg_6: Timestamp,
) -> Result(pog.Returned(CreateWorkspaceRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use organization_id <- decode.field(1, uuid_decoder())
    use name <- decode.field(2, decode.string)
    use slug <- decode.field(3, decode.string)
    use description <- decode.field(4, decode.optional(decode.string))
    use is_active <- decode.field(5, decode.optional(decode.bool))
    use created_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(7, decode.optional(pog.timestamp_decoder()))
    decode.success(CreateWorkspaceRow(
      id:,
      organization_id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Create a new workspace within an organization
INSERT INTO workspaces (organization_id, name, slug, description, created_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING id, organization_id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.text(arg_4))
  |> pog.parameter(pog.timestamp(arg_5))
  |> pog.parameter(pog.timestamp(arg_6))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Delete a chat (messages cascade delete)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn delete_chat(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Delete a chat (messages cascade delete)
DELETE FROM chats
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Delete a message by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn delete_message(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Delete a message by ID
DELETE FROM messages
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Delete an organization by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn delete_organization(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Delete an organization by ID
DELETE FROM organizations
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Delete a project by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn delete_project(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Delete a project by ID
DELETE FROM projects
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Delete a source by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn delete_source(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Delete a source by ID
DELETE FROM sources
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Delete a task by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn delete_task(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Delete a task by ID
DELETE FROM tasks
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Delete a workspace by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn delete_workspace(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Delete a workspace by ID
DELETE FROM workspaces
WHERE id = $1 AND organization_id = $2
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(uuid.to_string(arg_2)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Delete theme for a workspace (reset to defaults)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn delete_workspace_theme(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Delete theme for a workspace (reset to defaults)
DELETE FROM workspace_themes
WHERE workspace_id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Add a task to the execution queue (upsert)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn enqueue_task(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: Int,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Add a task to the execution queue (upsert)
INSERT INTO task_queue (task_id, priority)
VALUES ($1, $2)
ON CONFLICT (task_id) DO UPDATE SET priority = $2, queued_at = NOW()
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.int(arg_2))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_chat_by_id` query
/// defined in `./src/database/queries/sql/get_chat_by_id.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetChatByIdRow {
  GetChatByIdRow(
    id: Uuid,
    title: String,
    model_name: String,
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    archived: Option(Bool),
  )
}

/// Get a single chat by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_chat_by_id(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetChatByIdRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use title <- decode.field(1, decode.string)
    use model_name <- decode.field(2, decode.string)
    use created_at <- decode.field(3, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    use archived <- decode.field(5, decode.optional(decode.bool))
    decode.success(GetChatByIdRow(
      id:,
      title:,
      model_name:,
      created_at:,
      updated_at:,
      archived:,
    ))
  }

  "-- Get a single chat by ID
SELECT id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
FROM chats
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_message_by_id` query
/// defined in `./src/database/queries/sql/get_message_by_id.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetMessageByIdRow {
  GetMessageByIdRow(
    id: Uuid,
    chat_id: Uuid,
    role: String,
    content: String,
    created_at: Option(Timestamp),
  )
}

/// Get a single message by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_message_by_id(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetMessageByIdRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use chat_id <- decode.field(1, uuid_decoder())
    use role <- decode.field(2, decode.string)
    use content <- decode.field(3, decode.string)
    use created_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    decode.success(GetMessageByIdRow(
      id:,
      chat_id:,
      role:,
      content:,
      created_at:,
    ))
  }

  "-- Get a single message by ID
SELECT id, chat_id, role, content, created_at::timestamp FROM messages
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_organization_by_id` query
/// defined in `./src/database/queries/sql/get_organization_by_id.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetOrganizationByIdRow {
  GetOrganizationByIdRow(
    id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Get a single organization by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_organization_by_id(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetOrganizationByIdRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use slug <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.optional(decode.string))
    use is_active <- decode.field(4, decode.optional(decode.bool))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(GetOrganizationByIdRow(
      id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Get a single organization by ID
SELECT id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM organizations
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_organization_by_slug` query
/// defined in `./src/database/queries/sql/get_organization_by_slug.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetOrganizationBySlugRow {
  GetOrganizationBySlugRow(
    id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Get a single organization by slug
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_organization_by_slug(
  db: pog.Connection,
  arg_1: String,
) -> Result(pog.Returned(GetOrganizationBySlugRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use slug <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.optional(decode.string))
    use is_active <- decode.field(4, decode.optional(decode.bool))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(GetOrganizationBySlugRow(
      id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Get a single organization by slug
SELECT id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM organizations
WHERE slug = $1
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_project_by_id` query
/// defined in `./src/database/queries/sql/get_project_by_id.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetProjectByIdRow {
  GetProjectByIdRow(
    id: Uuid,
    name: String,
    description: Option(String),
    status: String,
    github_repo_url: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Get a single project by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_project_by_id(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetProjectByIdRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use description <- decode.field(2, decode.optional(decode.string))
    use status <- decode.field(3, decode.string)
    use github_repo_url <- decode.field(4, decode.optional(decode.string))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(GetProjectByIdRow(
      id:,
      name:,
      description:,
      status:,
      github_repo_url:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Get a single project by ID
SELECT id, name, description, status, github_repo_url, created_at::timestamp, updated_at::timestamp FROM projects
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_source_by_id` query
/// defined in `./src/database/queries/sql/get_source_by_id.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetSourceByIdRow {
  GetSourceByIdRow(
    id: Uuid,
    name: String,
    source_type: String,
    config: String,
    credentials_encrypted: Option(String),
    description: Option(String),
    url: Option(String),
    is_active: Option(Bool),
    last_verified_at: Option(Timestamp),
    last_error: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Get a single source by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_source_by_id(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetSourceByIdRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use source_type <- decode.field(2, decode.string)
    use config <- decode.field(3, decode.string)
    use credentials_encrypted <- decode.field(4, decode.optional(decode.string))
    use description <- decode.field(5, decode.optional(decode.string))
    use url <- decode.field(6, decode.optional(decode.string))
    use is_active <- decode.field(7, decode.optional(decode.bool))
    use last_verified_at <- decode.field(
      8,
      decode.optional(pog.timestamp_decoder()),
    )
    use last_error <- decode.field(9, decode.optional(decode.string))
    use created_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    decode.success(GetSourceByIdRow(
      id:,
      name:,
      source_type:,
      config:,
      credentials_encrypted:,
      description:,
      url:,
      is_active:,
      last_verified_at:,
      last_error:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Get a single source by ID
SELECT id, name, source_type, config, credentials_encrypted, description, url,
       is_active, last_verified_at::timestamp, last_error, created_at::timestamp, updated_at::timestamp FROM sources
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_task_by_id` query
/// defined in `./src/database/queries/sql/get_task_by_id.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetTaskByIdRow {
  GetTaskByIdRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// Get a single task by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_task_by_id(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetTaskByIdRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(GetTaskByIdRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- Get a single task by ID
SELECT id, project_id, title, description, acceptance_criteria, status,
       priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
FROM tasks
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_task_run_by_id` query
/// defined in `./src/database/queries/sql/get_task_run_by_id.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetTaskRunByIdRow {
  GetTaskRunByIdRow(
    id: Uuid,
    task_id: Uuid,
    status: String,
    current_phase: Option(String),
    progress_percent: Option(Int),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    error_message: Option(String),
  )
}

/// Get a task run by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_task_run_by_id(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetTaskRunByIdRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use task_id <- decode.field(1, uuid_decoder())
    use status <- decode.field(2, decode.string)
    use current_phase <- decode.field(3, decode.optional(decode.string))
    use progress_percent <- decode.field(4, decode.optional(decode.int))
    use started_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      6,
      decode.optional(pog.timestamp_decoder()),
    )
    use error_message <- decode.field(7, decode.optional(decode.string))
    decode.success(GetTaskRunByIdRow(
      id:,
      task_id:,
      status:,
      current_phase:,
      progress_percent:,
      started_at:,
      completed_at:,
      error_message:,
    ))
  }

  "-- Get a task run by ID
SELECT id, task_id, status, current_phase, progress_percent,
       started_at::timestamp, completed_at::timestamp, error_message
FROM task_runs
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_task_source` query
/// defined in `./src/database/queries/sql/get_task_source.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetTaskSourceRow {
  GetTaskSourceRow(
    id: Uuid,
    name: String,
    source_type: String,
    config: String,
    credentials_encrypted: Option(String),
    description: Option(String),
    url: Option(String),
    is_active: Option(Bool),
    last_verified_at: Option(Timestamp),
    last_error: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Get source for a task (task source or fallback to project source)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_task_source(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetTaskSourceRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use source_type <- decode.field(2, decode.string)
    use config <- decode.field(3, decode.string)
    use credentials_encrypted <- decode.field(4, decode.optional(decode.string))
    use description <- decode.field(5, decode.optional(decode.string))
    use url <- decode.field(6, decode.optional(decode.string))
    use is_active <- decode.field(7, decode.optional(decode.bool))
    use last_verified_at <- decode.field(
      8,
      decode.optional(pog.timestamp_decoder()),
    )
    use last_error <- decode.field(9, decode.optional(decode.string))
    use created_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    decode.success(GetTaskSourceRow(
      id:,
      name:,
      source_type:,
      config:,
      credentials_encrypted:,
      description:,
      url:,
      is_active:,
      last_verified_at:,
      last_error:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Get source for a task (task source or fallback to project source)
SELECT s.id, s.name, s.source_type, s.config, s.credentials_encrypted, s.description, s.url,
       s.is_active, s.last_verified_at::timestamp, s.last_error, s.created_at::timestamp, s.updated_at::timestamp FROM tasks t
JOIN projects p ON p.id = t.project_id
LEFT JOIN sources s ON s.id = COALESCE(t.source_id, p.source_id)
WHERE t.id = $1 AND s.is_active = TRUE
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_task_sources_array` query
/// defined in `./src/database/queries/sql/get_task_sources_array.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetTaskSourcesArrayRow {
  GetTaskSourcesArrayRow(
    id: Uuid,
    name: String,
    source_type: String,
    config: String,
    credentials_encrypted: Option(String),
    description: Option(String),
    url: Option(String),
    is_active: Option(Bool),
    last_verified_at: Option(Timestamp),
    last_error: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Get all sources for a task from source_ids array
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_task_sources_array(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetTaskSourcesArrayRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use source_type <- decode.field(2, decode.string)
    use config <- decode.field(3, decode.string)
    use credentials_encrypted <- decode.field(4, decode.optional(decode.string))
    use description <- decode.field(5, decode.optional(decode.string))
    use url <- decode.field(6, decode.optional(decode.string))
    use is_active <- decode.field(7, decode.optional(decode.bool))
    use last_verified_at <- decode.field(
      8,
      decode.optional(pog.timestamp_decoder()),
    )
    use last_error <- decode.field(9, decode.optional(decode.string))
    use created_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    decode.success(GetTaskSourcesArrayRow(
      id:,
      name:,
      source_type:,
      config:,
      credentials_encrypted:,
      description:,
      url:,
      is_active:,
      last_verified_at:,
      last_error:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Get all sources for a task from source_ids array
SELECT s.id, s.name, s.source_type, s.config, s.credentials_encrypted, s.description, s.url,
       s.is_active, s.last_verified_at::timestamp, s.last_error, s.created_at::timestamp, s.updated_at::timestamp FROM tasks t
CROSS JOIN LATERAL unnest(t.source_ids) AS task_source_id
JOIN sources s ON s.id = task_source_id
WHERE t.id = $1 AND s.is_active = TRUE
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_user_by_email` query
/// defined in `./src/database/queries/sql/get_user_by_email.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetUserByEmailRow {
  GetUserByEmailRow(
    id: Uuid,
    email: String,
    display_name: Option(String),
    is_active: Option(Bool),
    is_admin: Option(Bool),
    created_at: String,
    updated_at: String,
    last_login_at: String,
    password_hash: String,
  )
}

/// Get user by email (for login) - returns user with password hash
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_user_by_email(
  db: pog.Connection,
  arg_1: String,
) -> Result(pog.Returned(GetUserByEmailRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use email <- decode.field(1, decode.string)
    use display_name <- decode.field(2, decode.optional(decode.string))
    use is_active <- decode.field(3, decode.optional(decode.bool))
    use is_admin <- decode.field(4, decode.optional(decode.bool))
    use created_at <- decode.field(5, decode.string)
    use updated_at <- decode.field(6, decode.string)
    use last_login_at <- decode.field(7, decode.string)
    use password_hash <- decode.field(8, decode.string)
    decode.success(GetUserByEmailRow(
      id:,
      email:,
      display_name:,
      is_active:,
      is_admin:,
      created_at:,
      updated_at:,
      last_login_at:,
      password_hash:,
    ))
  }

  "-- Get user by email (for login) - returns user with password hash
SELECT id, email, display_name, is_active, is_admin,
       created_at::text, updated_at::text, COALESCE(last_login_at::text, '') AS last_login_at,
       password_hash
FROM users
WHERE email = $1
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_user_by_id` query
/// defined in `./src/database/queries/sql/get_user_by_id.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetUserByIdRow {
  GetUserByIdRow(
    id: Uuid,
    email: String,
    display_name: Option(String),
    is_active: Option(Bool),
    is_admin: Option(Bool),
    created_at: String,
    updated_at: String,
    last_login_at: String,
  )
}

/// Get user by ID
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_user_by_id(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetUserByIdRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use email <- decode.field(1, decode.string)
    use display_name <- decode.field(2, decode.optional(decode.string))
    use is_active <- decode.field(3, decode.optional(decode.bool))
    use is_admin <- decode.field(4, decode.optional(decode.bool))
    use created_at <- decode.field(5, decode.string)
    use updated_at <- decode.field(6, decode.string)
    use last_login_at <- decode.field(7, decode.string)
    decode.success(GetUserByIdRow(
      id:,
      email:,
      display_name:,
      is_active:,
      is_admin:,
      created_at:,
      updated_at:,
      last_login_at:,
    ))
  }

  "-- Get user by ID
SELECT id, email, display_name, is_active, is_admin,
       created_at::text, updated_at::text, COALESCE(last_login_at::text, '') AS last_login_at
FROM users
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_user_permissions` query
/// defined in `./src/database/queries/sql/get_user_permissions.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetUserPermissionsRow {
  GetUserPermissionsRow(name: String)
}

/// Get user's permission names (aggregated from all roles)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_user_permissions(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetUserPermissionsRow), pog.QueryError) {
  let decoder = {
    use name <- decode.field(0, decode.string)
    decode.success(GetUserPermissionsRow(name:))
  }

  "-- Get user's permission names (aggregated from all roles)
SELECT DISTINCT p.name
FROM permissions p
JOIN role_permissions rp ON rp.permission_id = p.id
JOIN user_roles ur ON ur.role_id = rp.role_id
WHERE ur.user_id = $1
ORDER BY p.name
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_user_roles` query
/// defined in `./src/database/queries/sql/get_user_roles.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetUserRolesRow {
  GetUserRolesRow(name: String)
}

/// Get user's role names
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_user_roles(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetUserRolesRow), pog.QueryError) {
  let decoder = {
    use name <- decode.field(0, decode.string)
    decode.success(GetUserRolesRow(name:))
  }

  "-- Get user's role names
SELECT r.name
FROM roles r
JOIN user_roles ur ON ur.role_id = r.id
WHERE ur.user_id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_workspace_by_id` query
/// defined in `./src/database/queries/sql/get_workspace_by_id.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetWorkspaceByIdRow {
  GetWorkspaceByIdRow(
    id: Uuid,
    organization_id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Get a single workspace by ID (verify it belongs to the organization)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_workspace_by_id(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: Uuid,
) -> Result(pog.Returned(GetWorkspaceByIdRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use organization_id <- decode.field(1, uuid_decoder())
    use name <- decode.field(2, decode.string)
    use slug <- decode.field(3, decode.string)
    use description <- decode.field(4, decode.optional(decode.string))
    use is_active <- decode.field(5, decode.optional(decode.bool))
    use created_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(7, decode.optional(pog.timestamp_decoder()))
    decode.success(GetWorkspaceByIdRow(
      id:,
      organization_id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Get a single workspace by ID (verify it belongs to the organization)
SELECT id, organization_id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM workspaces
WHERE id = $1 AND organization_id = $2
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(uuid.to_string(arg_2)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_workspace_by_slug` query
/// defined in `./src/database/queries/sql/get_workspace_by_slug.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetWorkspaceBySlugRow {
  GetWorkspaceBySlugRow(
    id: Uuid,
    organization_id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Get a single workspace by slug within an organization
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_workspace_by_slug(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: String,
) -> Result(pog.Returned(GetWorkspaceBySlugRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use organization_id <- decode.field(1, uuid_decoder())
    use name <- decode.field(2, decode.string)
    use slug <- decode.field(3, decode.string)
    use description <- decode.field(4, decode.optional(decode.string))
    use is_active <- decode.field(5, decode.optional(decode.bool))
    use created_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(7, decode.optional(pog.timestamp_decoder()))
    decode.success(GetWorkspaceBySlugRow(
      id:,
      organization_id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Get a single workspace by slug within an organization
SELECT id, organization_id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM workspaces
WHERE organization_id = $1 AND slug = $2
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(arg_2))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `get_workspace_theme` query
/// defined in `./src/database/queries/sql/get_workspace_theme.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type GetWorkspaceThemeRow {
  GetWorkspaceThemeRow(
    id: Uuid,
    workspace_id: Uuid,
    primary_color_light: Option(String),
    secondary_color_light: Option(String),
    primary_color_dark: Option(String),
    secondary_color_dark: Option(String),
    font_family: Option(String),
    font_size_base: Option(String),
    border_radius: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Get theme for a workspace
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn get_workspace_theme(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(GetWorkspaceThemeRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use workspace_id <- decode.field(1, uuid_decoder())
    use primary_color_light <- decode.field(2, decode.optional(decode.string))
    use secondary_color_light <- decode.field(3, decode.optional(decode.string))
    use primary_color_dark <- decode.field(4, decode.optional(decode.string))
    use secondary_color_dark <- decode.field(5, decode.optional(decode.string))
    use font_family <- decode.field(6, decode.optional(decode.string))
    use font_size_base <- decode.field(7, decode.optional(decode.string))
    use border_radius <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    decode.success(GetWorkspaceThemeRow(
      id:,
      workspace_id:,
      primary_color_light:,
      secondary_color_light:,
      primary_color_dark:,
      secondary_color_dark:,
      font_family:,
      font_size_base:,
      border_radius:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Get theme for a workspace
SELECT id, workspace_id, primary_color_light, secondary_color_light,
       primary_color_dark, secondary_color_dark, font_family, font_size_base,
       border_radius, created_at::timestamp, updated_at::timestamp FROM workspace_themes
WHERE workspace_id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `link_project_github` query
/// defined in `./src/database/queries/sql/link_project_github.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type LinkProjectGithubRow {
  LinkProjectGithubRow(
    id: Uuid,
    name: String,
    description: Option(String),
    status: String,
    github_repo_url: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Link a GitHub repository to a project
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn link_project_github(
  db: pog.Connection,
  arg_1: String,
  arg_2: Timestamp,
  arg_3: Uuid,
) -> Result(pog.Returned(LinkProjectGithubRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use description <- decode.field(2, decode.optional(decode.string))
    use status <- decode.field(3, decode.string)
    use github_repo_url <- decode.field(4, decode.optional(decode.string))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(LinkProjectGithubRow(
      id:,
      name:,
      description:,
      status:,
      github_repo_url:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Link a GitHub repository to a project
UPDATE projects
SET github_repo_url = $1, updated_at = $2
WHERE id = $3
RETURNING id, name, description, status, github_repo_url, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Link a source to a project
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn link_source_to_project(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: Timestamp,
  arg_3: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Link a source to a project
UPDATE projects
SET source_id = $1, updated_at = $2
WHERE id = $3
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Link a source to a task
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn link_source_to_task(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: Timestamp,
  arg_3: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Link a source to a task
UPDATE tasks
SET source_id = $1, updated_at = $2
WHERE id = $3
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Link multiple sources to a task
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn link_sources_to_task(
  db: pog.Connection,
  arg_1: List(Uuid),
  arg_2: Timestamp,
  arg_3: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Link multiple sources to a task
UPDATE tasks
SET source_ids = $1::uuid[], updated_at = $2
WHERE id = $3
"
  |> pog.query
  |> pog.parameter(
    pog.array(fn(value) { pog.text(uuid.to_string(value)) }, arg_1),
  )
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_chats_active` query
/// defined in `./src/database/queries/sql/list_chats_active.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListChatsActiveRow {
  ListChatsActiveRow(
    id: Uuid,
    title: String,
    model_name: String,
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    archived: Option(Bool),
  )
}

/// List non-archived chats ordered by updated_at::timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_chats_active(
  db: pog.Connection,
) -> Result(pog.Returned(ListChatsActiveRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use title <- decode.field(1, decode.string)
    use model_name <- decode.field(2, decode.string)
    use created_at <- decode.field(3, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    use archived <- decode.field(5, decode.optional(decode.bool))
    decode.success(ListChatsActiveRow(
      id:,
      title:,
      model_name:,
      created_at:,
      updated_at:,
      archived:,
    ))
  }

  "-- List non-archived chats ordered by updated_at::timestamp
SELECT id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
FROM chats
WHERE archived = false
ORDER BY updated_at DESC
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_chats_all` query
/// defined in `./src/database/queries/sql/list_chats_all.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListChatsAllRow {
  ListChatsAllRow(
    id: Uuid,
    title: String,
    model_name: String,
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    archived: Option(Bool),
  )
}

/// List all chats ordered by updated_at
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_chats_all(
  db: pog.Connection,
) -> Result(pog.Returned(ListChatsAllRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use title <- decode.field(1, decode.string)
    use model_name <- decode.field(2, decode.string)
    use created_at <- decode.field(3, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    use archived <- decode.field(5, decode.optional(decode.bool))
    decode.success(ListChatsAllRow(
      id:,
      title:,
      model_name:,
      created_at:,
      updated_at:,
      archived:,
    ))
  }

  "-- List all chats ordered by updated_at
SELECT id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
FROM chats
ORDER BY updated_at DESC
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_chats_archived` query
/// defined in `./src/database/queries/sql/list_chats_archived.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListChatsArchivedRow {
  ListChatsArchivedRow(
    id: Uuid,
    title: String,
    model_name: String,
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    archived: Option(Bool),
  )
}

/// List archived chats ordered by updated_at::timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_chats_archived(
  db: pog.Connection,
) -> Result(pog.Returned(ListChatsArchivedRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use title <- decode.field(1, decode.string)
    use model_name <- decode.field(2, decode.string)
    use created_at <- decode.field(3, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    use archived <- decode.field(5, decode.optional(decode.bool))
    decode.success(ListChatsArchivedRow(
      id:,
      title:,
      model_name:,
      created_at:,
      updated_at:,
      archived:,
    ))
  }

  "-- List archived chats ordered by updated_at::timestamp
SELECT id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
FROM chats
WHERE archived = true
ORDER BY updated_at DESC
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_messages` query
/// defined in `./src/database/queries/sql/list_messages.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListMessagesRow {
  ListMessagesRow(
    id: Uuid,
    chat_id: Uuid,
    role: String,
    content: String,
    created_at: Option(Timestamp),
  )
}

/// List messages for a chat ordered by created_at::timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_messages(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(ListMessagesRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use chat_id <- decode.field(1, uuid_decoder())
    use role <- decode.field(2, decode.string)
    use content <- decode.field(3, decode.string)
    use created_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    decode.success(ListMessagesRow(id:, chat_id:, role:, content:, created_at:))
  }

  "-- List messages for a chat ordered by created_at::timestamp
SELECT id, chat_id, role, content, created_at::timestamp FROM messages
WHERE chat_id = $1
ORDER BY created_at ASC
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_organizations_active` query
/// defined in `./src/database/queries/sql/list_organizations_active.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListOrganizationsActiveRow {
  ListOrganizationsActiveRow(
    id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// List only active organizations ordered by name
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_organizations_active(
  db: pog.Connection,
) -> Result(pog.Returned(ListOrganizationsActiveRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use slug <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.optional(decode.string))
    use is_active <- decode.field(4, decode.optional(decode.bool))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(ListOrganizationsActiveRow(
      id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- List only active organizations ordered by name
SELECT id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM organizations
WHERE is_active = true
ORDER BY name ASC
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_organizations_all` query
/// defined in `./src/database/queries/sql/list_organizations_all.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListOrganizationsAllRow {
  ListOrganizationsAllRow(
    id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// List all organizations ordered by name
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_organizations_all(
  db: pog.Connection,
) -> Result(pog.Returned(ListOrganizationsAllRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use slug <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.optional(decode.string))
    use is_active <- decode.field(4, decode.optional(decode.bool))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(ListOrganizationsAllRow(
      id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- List all organizations ordered by name
SELECT id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM organizations
ORDER BY name ASC
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_projects_all` query
/// defined in `./src/database/queries/sql/list_projects_all.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListProjectsAllRow {
  ListProjectsAllRow(
    id: Uuid,
    name: String,
    description: Option(String),
    status: String,
    github_repo_url: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// List all projects ordered by updated_at::timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_projects_all(
  db: pog.Connection,
) -> Result(pog.Returned(ListProjectsAllRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use description <- decode.field(2, decode.optional(decode.string))
    use status <- decode.field(3, decode.string)
    use github_repo_url <- decode.field(4, decode.optional(decode.string))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(ListProjectsAllRow(
      id:,
      name:,
      description:,
      status:,
      github_repo_url:,
      created_at:,
      updated_at:,
    ))
  }

  "-- List all projects ordered by updated_at::timestamp
SELECT id, name, description, status, github_repo_url, created_at::timestamp, updated_at::timestamp FROM projects
ORDER BY updated_at DESC
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_projects_by_status` query
/// defined in `./src/database/queries/sql/list_projects_by_status.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListProjectsByStatusRow {
  ListProjectsByStatusRow(
    id: Uuid,
    name: String,
    description: Option(String),
    status: String,
    github_repo_url: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// List projects filtered by status ordered by updated_at::timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_projects_by_status(
  db: pog.Connection,
  arg_1: String,
) -> Result(pog.Returned(ListProjectsByStatusRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use description <- decode.field(2, decode.optional(decode.string))
    use status <- decode.field(3, decode.string)
    use github_repo_url <- decode.field(4, decode.optional(decode.string))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(ListProjectsByStatusRow(
      id:,
      name:,
      description:,
      status:,
      github_repo_url:,
      created_at:,
      updated_at:,
    ))
  }

  "-- List projects filtered by status ordered by updated_at::timestamp
SELECT id, name, description, status, github_repo_url, created_at::timestamp, updated_at::timestamp FROM projects
WHERE status = $1
ORDER BY updated_at DESC
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_queued_tasks` query
/// defined in `./src/database/queries/sql/list_queued_tasks.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListQueuedTasksRow {
  ListQueuedTasksRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// Get queued tasks for display/monitoring
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_queued_tasks(
  db: pog.Connection,
) -> Result(pog.Returned(ListQueuedTasksRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(ListQueuedTasksRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- Get queued tasks for display/monitoring
SELECT t.id, t.project_id, t.title, t.description, t.acceptance_criteria, t.status,
       t.priority, t.model_name, t.dependencies, t.created_at::timestamp, t.updated_at::timestamp, t.started_at::timestamp, t.completed_at::timestamp, t.is_agentic, t.github_repo_url, t.queued_at::timestamp, t.worker_id
FROM tasks t
JOIN task_queue tq ON tq.task_id = t.id
ORDER BY tq.priority DESC, tq.queued_at ASC
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_sources_by_category` query
/// defined in `./src/database/queries/sql/list_sources_by_category.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListSourcesByCategoryRow {
  ListSourcesByCategoryRow(
    id: Uuid,
    name: String,
    source_type: String,
    config: String,
    credentials_encrypted: Option(String),
    description: Option(String),
    url: Option(String),
    is_active: Option(Bool),
    last_verified_at: Option(Timestamp),
    last_error: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// List sources by category (active only)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_sources_by_category(
  db: pog.Connection,
  arg_1: String,
) -> Result(pog.Returned(ListSourcesByCategoryRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use source_type <- decode.field(2, decode.string)
    use config <- decode.field(3, decode.string)
    use credentials_encrypted <- decode.field(4, decode.optional(decode.string))
    use description <- decode.field(5, decode.optional(decode.string))
    use url <- decode.field(6, decode.optional(decode.string))
    use is_active <- decode.field(7, decode.optional(decode.bool))
    use last_verified_at <- decode.field(
      8,
      decode.optional(pog.timestamp_decoder()),
    )
    use last_error <- decode.field(9, decode.optional(decode.string))
    use created_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    decode.success(ListSourcesByCategoryRow(
      id:,
      name:,
      source_type:,
      config:,
      credentials_encrypted:,
      description:,
      url:,
      is_active:,
      last_verified_at:,
      last_error:,
      created_at:,
      updated_at:,
    ))
  }

  "-- List sources by category (active only)
SELECT s.id, s.name, s.source_type, s.config, s.credentials_encrypted, s.description, s.url,
       s.is_active, s.last_verified_at::timestamp, s.last_error, s.created_at::timestamp, s.updated_at::timestamp FROM sources s
JOIN source_types st ON st.name = s.source_type
WHERE st.category = $1 AND s.is_active = TRUE
ORDER BY s.name ASC
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_sources_by_category_all` query
/// defined in `./src/database/queries/sql/list_sources_by_category_all.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListSourcesByCategoryAllRow {
  ListSourcesByCategoryAllRow(
    id: Uuid,
    name: String,
    source_type: String,
    config: String,
    credentials_encrypted: Option(String),
    description: Option(String),
    url: Option(String),
    is_active: Option(Bool),
    last_verified_at: Option(Timestamp),
    last_error: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// List sources by category (all)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_sources_by_category_all(
  db: pog.Connection,
  arg_1: String,
) -> Result(pog.Returned(ListSourcesByCategoryAllRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use source_type <- decode.field(2, decode.string)
    use config <- decode.field(3, decode.string)
    use credentials_encrypted <- decode.field(4, decode.optional(decode.string))
    use description <- decode.field(5, decode.optional(decode.string))
    use url <- decode.field(6, decode.optional(decode.string))
    use is_active <- decode.field(7, decode.optional(decode.bool))
    use last_verified_at <- decode.field(
      8,
      decode.optional(pog.timestamp_decoder()),
    )
    use last_error <- decode.field(9, decode.optional(decode.string))
    use created_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    decode.success(ListSourcesByCategoryAllRow(
      id:,
      name:,
      source_type:,
      config:,
      credentials_encrypted:,
      description:,
      url:,
      is_active:,
      last_verified_at:,
      last_error:,
      created_at:,
      updated_at:,
    ))
  }

  "-- List sources by category (all)
SELECT s.id, s.name, s.source_type, s.config, s.credentials_encrypted, s.description, s.url,
       s.is_active, s.last_verified_at::timestamp, s.last_error, s.created_at::timestamp, s.updated_at::timestamp FROM sources s
JOIN source_types st ON st.name = s.source_type
WHERE st.category = $1
ORDER BY s.name ASC
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_task_run_logs` query
/// defined in `./src/database/queries/sql/list_task_run_logs.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListTaskRunLogsRow {
  ListTaskRunLogsRow(
    id: Uuid,
    task_run_id: Uuid,
    phase: String,
    agent_type: String,
    log_level: String,
    message: String,
    created_at: Option(Timestamp),
  )
}

/// List logs for a task run ordered by created_at::timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_task_run_logs(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(ListTaskRunLogsRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use task_run_id <- decode.field(1, uuid_decoder())
    use phase <- decode.field(2, decode.string)
    use agent_type <- decode.field(3, decode.string)
    use log_level <- decode.field(4, decode.string)
    use message <- decode.field(5, decode.string)
    use created_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(ListTaskRunLogsRow(
      id:,
      task_run_id:,
      phase:,
      agent_type:,
      log_level:,
      message:,
      created_at:,
    ))
  }

  "-- List logs for a task run ordered by created_at::timestamp
SELECT id, task_run_id, phase, agent_type, log_level, message, created_at::timestamp FROM task_run_logs
WHERE task_run_id = $1
ORDER BY created_at ASC
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_task_runs` query
/// defined in `./src/database/queries/sql/list_task_runs.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListTaskRunsRow {
  ListTaskRunsRow(
    id: Uuid,
    task_id: Uuid,
    status: String,
    current_phase: Option(String),
    progress_percent: Option(Int),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    error_message: Option(String),
  )
}

/// List runs for a task ordered by started_at descending
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_task_runs(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(ListTaskRunsRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use task_id <- decode.field(1, uuid_decoder())
    use status <- decode.field(2, decode.string)
    use current_phase <- decode.field(3, decode.optional(decode.string))
    use progress_percent <- decode.field(4, decode.optional(decode.int))
    use started_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      6,
      decode.optional(pog.timestamp_decoder()),
    )
    use error_message <- decode.field(7, decode.optional(decode.string))
    decode.success(ListTaskRunsRow(
      id:,
      task_id:,
      status:,
      current_phase:,
      progress_percent:,
      started_at:,
      completed_at:,
      error_message:,
    ))
  }

  "-- List runs for a task ordered by started_at descending
SELECT id, task_id, status, current_phase, progress_percent,
       started_at::timestamp, completed_at::timestamp, error_message
FROM task_runs
WHERE task_id = $1
ORDER BY started_at DESC
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_tasks_all` query
/// defined in `./src/database/queries/sql/list_tasks_all.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListTasksAllRow {
  ListTasksAllRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// List all tasks ordered by priority and updated_at::timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_tasks_all(
  db: pog.Connection,
) -> Result(pog.Returned(ListTasksAllRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(ListTasksAllRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- List all tasks ordered by priority and updated_at::timestamp
SELECT id, project_id, title, description, acceptance_criteria, status,
       priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
FROM tasks
ORDER BY priority ASC, updated_at DESC
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_tasks_by_project` query
/// defined in `./src/database/queries/sql/list_tasks_by_project.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListTasksByProjectRow {
  ListTasksByProjectRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// List tasks for a specific project ordered by priority and updated_at::timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_tasks_by_project(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(ListTasksByProjectRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(ListTasksByProjectRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- List tasks for a specific project ordered by priority and updated_at::timestamp
SELECT id, project_id, title, description, acceptance_criteria, status,
       priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
FROM tasks
WHERE project_id = $1
ORDER BY priority ASC, updated_at DESC
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_tasks_by_project_and_status` query
/// defined in `./src/database/queries/sql/list_tasks_by_project_and_status.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListTasksByProjectAndStatusRow {
  ListTasksByProjectAndStatusRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// List tasks filtered by project and status ordered by priority and updated_at::timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_tasks_by_project_and_status(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: String,
) -> Result(pog.Returned(ListTasksByProjectAndStatusRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(ListTasksByProjectAndStatusRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- List tasks filtered by project and status ordered by priority and updated_at::timestamp
SELECT id, project_id, title, description, acceptance_criteria, status,
       priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
FROM tasks
WHERE project_id = $1 AND status = $2
ORDER BY priority ASC, updated_at DESC
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(arg_2))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_tasks_by_status` query
/// defined in `./src/database/queries/sql/list_tasks_by_status.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListTasksByStatusRow {
  ListTasksByStatusRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// List tasks filtered by status ordered by priority and updated_at::timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_tasks_by_status(
  db: pog.Connection,
  arg_1: String,
) -> Result(pog.Returned(ListTasksByStatusRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(ListTasksByStatusRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- List tasks filtered by status ordered by priority and updated_at::timestamp
SELECT id, project_id, title, description, acceptance_criteria, status,
       priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
FROM tasks
WHERE status = $1
ORDER BY priority ASC, updated_at DESC
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_users` query
/// defined in `./src/database/queries/sql/list_users.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListUsersRow {
  ListUsersRow(
    id: Uuid,
    email: String,
    display_name: Option(String),
    is_active: Option(Bool),
    is_admin: Option(Bool),
    created_at: String,
    updated_at: String,
    last_login_at: String,
  )
}

/// List all users
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_users(
  db: pog.Connection,
) -> Result(pog.Returned(ListUsersRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use email <- decode.field(1, decode.string)
    use display_name <- decode.field(2, decode.optional(decode.string))
    use is_active <- decode.field(3, decode.optional(decode.bool))
    use is_admin <- decode.field(4, decode.optional(decode.bool))
    use created_at <- decode.field(5, decode.string)
    use updated_at <- decode.field(6, decode.string)
    use last_login_at <- decode.field(7, decode.string)
    decode.success(ListUsersRow(
      id:,
      email:,
      display_name:,
      is_active:,
      is_admin:,
      created_at:,
      updated_at:,
      last_login_at:,
    ))
  }

  "-- List all users
SELECT id, email, display_name, is_active, is_admin,
       created_at::text, updated_at::text, COALESCE(last_login_at::text, '') AS last_login_at
FROM users
ORDER BY created_at DESC
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_workspaces_active` query
/// defined in `./src/database/queries/sql/list_workspaces_active.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListWorkspacesActiveRow {
  ListWorkspacesActiveRow(
    id: Uuid,
    organization_id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// List only active workspaces for an organization ordered by name
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_workspaces_active(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(ListWorkspacesActiveRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use organization_id <- decode.field(1, uuid_decoder())
    use name <- decode.field(2, decode.string)
    use slug <- decode.field(3, decode.string)
    use description <- decode.field(4, decode.optional(decode.string))
    use is_active <- decode.field(5, decode.optional(decode.bool))
    use created_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(7, decode.optional(pog.timestamp_decoder()))
    decode.success(ListWorkspacesActiveRow(
      id:,
      organization_id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- List only active workspaces for an organization ordered by name
SELECT id, organization_id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM workspaces
WHERE organization_id = $1 AND is_active = true
ORDER BY name ASC
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `list_workspaces_all` query
/// defined in `./src/database/queries/sql/list_workspaces_all.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ListWorkspacesAllRow {
  ListWorkspacesAllRow(
    id: Uuid,
    organization_id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// List all workspaces for an organization ordered by name
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn list_workspaces_all(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(ListWorkspacesAllRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use organization_id <- decode.field(1, uuid_decoder())
    use name <- decode.field(2, decode.string)
    use slug <- decode.field(3, decode.string)
    use description <- decode.field(4, decode.optional(decode.string))
    use is_active <- decode.field(5, decode.optional(decode.bool))
    use created_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(7, decode.optional(pog.timestamp_decoder()))
    decode.success(ListWorkspacesAllRow(
      id:,
      organization_id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- List all workspaces for an organization ordered by name
SELECT id, organization_id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM workspaces
WHERE organization_id = $1
ORDER BY name ASC
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `recover_orphaned_tasks` query
/// defined in `./src/database/queries/sql/recover_orphaned_tasks.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type RecoverOrphanedTasksRow {
  RecoverOrphanedTasksRow(recover_orphaned_tasks: Int)
}

/// Recover orphaned tasks (called on worker startup)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn recover_orphaned_tasks(
  db: pog.Connection,
) -> Result(pog.Returned(RecoverOrphanedTasksRow), pog.QueryError) {
  let decoder = {
    use recover_orphaned_tasks <- decode.field(0, decode.int)
    decode.success(RecoverOrphanedTasksRow(recover_orphaned_tasks:))
  }

  "-- Recover orphaned tasks (called on worker startup)
SELECT recover_orphaned_tasks()
"
  |> pog.query
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `release_task` query
/// defined in `./src/database/queries/sql/release_task.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ReleaseTaskRow {
  ReleaseTaskRow(success: Bool)
}

/// Release a task back to the queue
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn release_task(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: String,
) -> Result(pog.Returned(ReleaseTaskRow), pog.QueryError) {
  let decoder = {
    use success <- decode.field(0, decode.bool)
    decode.success(ReleaseTaskRow(success:))
  }

  "-- Release a task back to the queue
SELECT (release_task($1, $2) IS NULL) AS success
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(arg_2))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Remove a role from a user
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn remove_user_role(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: String,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Remove a role from a user
DELETE FROM user_roles
WHERE user_id = $1 AND role_id = (SELECT id FROM roles WHERE name = $2)
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(arg_2))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Revoke all refresh tokens for a user (logout everywhere)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn revoke_all_user_tokens(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Revoke all refresh tokens for a user (logout everywhere)
UPDATE refresh_tokens
SET revoked_at = NOW()
WHERE user_id = $1 AND revoked_at IS NULL
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Revoke a refresh token
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn revoke_refresh_token(
  db: pog.Connection,
  arg_1: String,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Revoke a refresh token
UPDATE refresh_tokens
SET revoked_at = NOW()
WHERE token_hash = $1 AND revoked_at IS NULL
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Update user active status
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn set_user_active(
  db: pog.Connection,
  arg_1: Bool,
  arg_2: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Update user active status
UPDATE users
SET is_active = $1, updated_at = NOW()
WHERE id = $2
"
  |> pog.query
  |> pog.parameter(pog.bool(arg_1))
  |> pog.parameter(pog.text(uuid.to_string(arg_2)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Update chat's updated_at timestamp
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn touch_chat(
  db: pog.Connection,
  arg_1: Timestamp,
  arg_2: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Update chat's updated_at timestamp
UPDATE chats
SET updated_at = $1
WHERE id = $2
"
  |> pog.query
  |> pog.parameter(pog.timestamp(arg_1))
  |> pog.parameter(pog.text(uuid.to_string(arg_2)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `unarchive_chat` query
/// defined in `./src/database/queries/sql/unarchive_chat.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UnarchiveChatRow {
  UnarchiveChatRow(
    id: Uuid,
    title: String,
    model_name: String,
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    archived: Option(Bool),
  )
}

/// Unarchive a chat
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn unarchive_chat(
  db: pog.Connection,
  arg_1: Timestamp,
  arg_2: Uuid,
) -> Result(pog.Returned(UnarchiveChatRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use title <- decode.field(1, decode.string)
    use model_name <- decode.field(2, decode.string)
    use created_at <- decode.field(3, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    use archived <- decode.field(5, decode.optional(decode.bool))
    decode.success(UnarchiveChatRow(
      id:,
      title:,
      model_name:,
      created_at:,
      updated_at:,
      archived:,
    ))
  }

  "-- Unarchive a chat
UPDATE chats
SET archived = false, updated_at = $1
WHERE id = $2
RETURNING id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
"
  |> pog.query
  |> pog.parameter(pog.timestamp(arg_1))
  |> pog.parameter(pog.text(uuid.to_string(arg_2)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `unassign_task_worker` query
/// defined in `./src/database/queries/sql/unassign_task_worker.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UnassignTaskWorkerRow {
  UnassignTaskWorkerRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// Clear worker assignment from task
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn unassign_task_worker(
  db: pog.Connection,
  arg_1: Timestamp,
  arg_2: Uuid,
) -> Result(pog.Returned(UnassignTaskWorkerRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(UnassignTaskWorkerRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- Clear worker assignment from task
UPDATE tasks
SET worker_id = NULL, updated_at = $1
WHERE id = $2
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
"
  |> pog.query
  |> pog.parameter(pog.timestamp(arg_1))
  |> pog.parameter(pog.text(uuid.to_string(arg_2)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `unlink_project_github` query
/// defined in `./src/database/queries/sql/unlink_project_github.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UnlinkProjectGithubRow {
  UnlinkProjectGithubRow(
    id: Uuid,
    name: String,
    description: Option(String),
    status: String,
    github_repo_url: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Unlink GitHub repository from a project
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn unlink_project_github(
  db: pog.Connection,
  arg_1: Timestamp,
  arg_2: Uuid,
) -> Result(pog.Returned(UnlinkProjectGithubRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use description <- decode.field(2, decode.optional(decode.string))
    use status <- decode.field(3, decode.string)
    use github_repo_url <- decode.field(4, decode.optional(decode.string))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(UnlinkProjectGithubRow(
      id:,
      name:,
      description:,
      status:,
      github_repo_url:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Unlink GitHub repository from a project
UPDATE projects
SET github_repo_url = NULL, updated_at = $1
WHERE id = $2
RETURNING id, name, description, status, github_repo_url, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.timestamp(arg_1))
  |> pog.parameter(pog.text(uuid.to_string(arg_2)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Unlink source from a project
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn unlink_source_from_project(
  db: pog.Connection,
  arg_1: Timestamp,
  arg_2: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Unlink source from a project
UPDATE projects
SET source_id = NULL, updated_at = $1
WHERE id = $2
"
  |> pog.query
  |> pog.parameter(pog.timestamp(arg_1))
  |> pog.parameter(pog.text(uuid.to_string(arg_2)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_chat_title` query
/// defined in `./src/database/queries/sql/update_chat_title.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateChatTitleRow {
  UpdateChatTitleRow(
    id: Uuid,
    title: String,
    model_name: String,
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    archived: Option(Bool),
  )
}

/// Update chat title
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_chat_title(
  db: pog.Connection,
  arg_1: String,
  arg_2: Timestamp,
  arg_3: Uuid,
) -> Result(pog.Returned(UpdateChatTitleRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use title <- decode.field(1, decode.string)
    use model_name <- decode.field(2, decode.string)
    use created_at <- decode.field(3, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(4, decode.optional(pog.timestamp_decoder()))
    use archived <- decode.field(5, decode.optional(decode.bool))
    decode.success(UpdateChatTitleRow(
      id:,
      title:,
      model_name:,
      created_at:,
      updated_at:,
      archived:,
    ))
  }

  "-- Update chat title
UPDATE chats
SET title = $1, updated_at = $2
WHERE id = $3
RETURNING id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_organization` query
/// defined in `./src/database/queries/sql/update_organization.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateOrganizationRow {
  UpdateOrganizationRow(
    id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Update an existing organization
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_organization(
  db: pog.Connection,
  arg_1: String,
  arg_2: String,
  arg_3: String,
  arg_4: Bool,
  arg_5: Timestamp,
  arg_6: Uuid,
) -> Result(pog.Returned(UpdateOrganizationRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use slug <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.optional(decode.string))
    use is_active <- decode.field(4, decode.optional(decode.bool))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(UpdateOrganizationRow(
      id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Update an existing organization
UPDATE organizations
SET name = $1, slug = $2, description = $3, is_active = $4, updated_at = $5
WHERE id = $6
RETURNING id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.bool(arg_4))
  |> pog.parameter(pog.timestamp(arg_5))
  |> pog.parameter(pog.text(uuid.to_string(arg_6)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_project` query
/// defined in `./src/database/queries/sql/update_project.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateProjectRow {
  UpdateProjectRow(
    id: Uuid,
    name: String,
    description: Option(String),
    status: String,
    github_repo_url: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Update an existing project
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_project(
  db: pog.Connection,
  arg_1: String,
  arg_2: String,
  arg_3: String,
  arg_4: String,
  arg_5: Timestamp,
  arg_6: Uuid,
) -> Result(pog.Returned(UpdateProjectRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use description <- decode.field(2, decode.optional(decode.string))
    use status <- decode.field(3, decode.string)
    use github_repo_url <- decode.field(4, decode.optional(decode.string))
    use created_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    decode.success(UpdateProjectRow(
      id:,
      name:,
      description:,
      status:,
      github_repo_url:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Update an existing project
UPDATE projects
SET name = $1, description = $2, status = $3, github_repo_url = $4, updated_at = $5
WHERE id = $6
RETURNING id, name, description, status, github_repo_url, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.text(arg_4))
  |> pog.parameter(pog.timestamp(arg_5))
  |> pog.parameter(pog.text(uuid.to_string(arg_6)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_source` query
/// defined in `./src/database/queries/sql/update_source.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateSourceRow {
  UpdateSourceRow(
    id: Uuid,
    name: String,
    source_type: String,
    config: String,
    credentials_encrypted: Option(String),
    description: Option(String),
    url: Option(String),
    is_active: Option(Bool),
    last_verified_at: Option(Timestamp),
    last_error: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Update an existing source
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_source(
  db: pog.Connection,
  arg_1: String,
  arg_2: Json,
  arg_3: String,
  arg_4: String,
  arg_5: String,
  arg_6: Bool,
  arg_7: Timestamp,
  arg_8: Uuid,
) -> Result(pog.Returned(UpdateSourceRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use name <- decode.field(1, decode.string)
    use source_type <- decode.field(2, decode.string)
    use config <- decode.field(3, decode.string)
    use credentials_encrypted <- decode.field(4, decode.optional(decode.string))
    use description <- decode.field(5, decode.optional(decode.string))
    use url <- decode.field(6, decode.optional(decode.string))
    use is_active <- decode.field(7, decode.optional(decode.bool))
    use last_verified_at <- decode.field(
      8,
      decode.optional(pog.timestamp_decoder()),
    )
    use last_error <- decode.field(9, decode.optional(decode.string))
    use created_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    decode.success(UpdateSourceRow(
      id:,
      name:,
      source_type:,
      config:,
      credentials_encrypted:,
      description:,
      url:,
      is_active:,
      last_verified_at:,
      last_error:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Update an existing source
UPDATE sources
SET name = $1, config = $2::jsonb, credentials_encrypted = $3,
    description = $4, url = $5, is_active = $6, updated_at = $7
WHERE id = $8
RETURNING id, name, source_type, config, credentials_encrypted, description, url,
          is_active, last_verified_at::timestamp, last_error, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.text(json.to_string(arg_2)))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.text(arg_4))
  |> pog.parameter(pog.text(arg_5))
  |> pog.parameter(pog.bool(arg_6))
  |> pog.parameter(pog.timestamp(arg_7))
  |> pog.parameter(pog.text(uuid.to_string(arg_8)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_task` query
/// defined in `./src/database/queries/sql/update_task.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateTaskRow {
  UpdateTaskRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// Update an existing task
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_task(
  db: pog.Connection,
  arg_1: String,
  arg_2: String,
  arg_3: String,
  arg_4: String,
  arg_5: Int,
  arg_6: String,
  arg_7: Json,
  arg_8: Bool,
  arg_9: String,
  arg_10: Timestamp,
  arg_11: Uuid,
) -> Result(pog.Returned(UpdateTaskRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(UpdateTaskRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- Update an existing task
UPDATE tasks
SET title = $1, description = $2, acceptance_criteria = $3,
    status = $4, priority = $5, model_name = $6, dependencies = $7,
    is_agentic = $8, github_repo_url = $9, updated_at = $10
WHERE id = $11
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.text(arg_4))
  |> pog.parameter(pog.int(arg_5))
  |> pog.parameter(pog.text(arg_6))
  |> pog.parameter(pog.text(json.to_string(arg_7)))
  |> pog.parameter(pog.bool(arg_8))
  |> pog.parameter(pog.text(arg_9))
  |> pog.parameter(pog.timestamp(arg_10))
  |> pog.parameter(pog.text(uuid.to_string(arg_11)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_task_run_progress` query
/// defined in `./src/database/queries/sql/update_task_run_progress.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateTaskRunProgressRow {
  UpdateTaskRunProgressRow(
    id: Uuid,
    task_id: Uuid,
    status: String,
    current_phase: Option(String),
    progress_percent: Option(Int),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    error_message: Option(String),
  )
}

/// Update task run progress
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_task_run_progress(
  db: pog.Connection,
  arg_1: String,
  arg_2: Int,
  arg_3: Uuid,
) -> Result(pog.Returned(UpdateTaskRunProgressRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use task_id <- decode.field(1, uuid_decoder())
    use status <- decode.field(2, decode.string)
    use current_phase <- decode.field(3, decode.optional(decode.string))
    use progress_percent <- decode.field(4, decode.optional(decode.int))
    use started_at <- decode.field(5, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      6,
      decode.optional(pog.timestamp_decoder()),
    )
    use error_message <- decode.field(7, decode.optional(decode.string))
    decode.success(UpdateTaskRunProgressRow(
      id:,
      task_id:,
      status:,
      current_phase:,
      progress_percent:,
      started_at:,
      completed_at:,
      error_message:,
    ))
  }

  "-- Update task run progress
UPDATE task_runs
SET current_phase = $1, progress_percent = $2
WHERE id = $3
RETURNING id, task_id, status, current_phase, progress_percent,
          started_at::timestamp, completed_at::timestamp, error_message
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.int(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_task_status_complete` query
/// defined in `./src/database/queries/sql/update_task_status_complete.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateTaskStatusCompleteRow {
  UpdateTaskStatusCompleteRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// Update task status to complete
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_task_status_complete(
  db: pog.Connection,
  arg_1: String,
  arg_2: Timestamp,
  arg_3: Uuid,
) -> Result(pog.Returned(UpdateTaskStatusCompleteRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(UpdateTaskStatusCompleteRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- Update task status to complete
UPDATE tasks
SET status = $1, completed_at = $2, updated_at = $2
WHERE id = $3
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_task_status_generic` query
/// defined in `./src/database/queries/sql/update_task_status_generic.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateTaskStatusGenericRow {
  UpdateTaskStatusGenericRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// Update task status (generic)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_task_status_generic(
  db: pog.Connection,
  arg_1: String,
  arg_2: Timestamp,
  arg_3: Uuid,
) -> Result(pog.Returned(UpdateTaskStatusGenericRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(UpdateTaskStatusGenericRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- Update task status (generic)
UPDATE tasks
SET status = $1, updated_at = $2
WHERE id = $3
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_task_status_in_progress` query
/// defined in `./src/database/queries/sql/update_task_status_in_progress.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateTaskStatusInProgressRow {
  UpdateTaskStatusInProgressRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// Update task status to in_progress
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_task_status_in_progress(
  db: pog.Connection,
  arg_1: String,
  arg_2: Timestamp,
  arg_3: Uuid,
) -> Result(pog.Returned(UpdateTaskStatusInProgressRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(UpdateTaskStatusInProgressRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- Update task status to in_progress
UPDATE tasks
SET status = $1, started_at = COALESCE(started_at::timestamp, $2), updated_at = $2
WHERE id = $3
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_task_status_queued` query
/// defined in `./src/database/queries/sql/update_task_status_queued.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateTaskStatusQueuedRow {
  UpdateTaskStatusQueuedRow(
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    acceptance_criteria: Option(String),
    status: String,
    priority: Option(Int),
    model_name: Option(String),
    dependencies: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
    started_at: Option(Timestamp),
    completed_at: Option(Timestamp),
    is_agentic: Bool,
    github_repo_url: Option(String),
    queued_at: Option(Timestamp),
    worker_id: Option(String),
  )
}

/// Update task status to queued
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_task_status_queued(
  db: pog.Connection,
  arg_1: String,
  arg_2: Timestamp,
  arg_3: Uuid,
) -> Result(pog.Returned(UpdateTaskStatusQueuedRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use project_id <- decode.field(1, uuid_decoder())
    use title <- decode.field(2, decode.string)
    use description <- decode.field(3, decode.string)
    use acceptance_criteria <- decode.field(4, decode.optional(decode.string))
    use status <- decode.field(5, decode.string)
    use priority <- decode.field(6, decode.optional(decode.int))
    use model_name <- decode.field(7, decode.optional(decode.string))
    use dependencies <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    use started_at <- decode.field(11, decode.optional(pog.timestamp_decoder()))
    use completed_at <- decode.field(
      12,
      decode.optional(pog.timestamp_decoder()),
    )
    use is_agentic <- decode.field(13, decode.bool)
    use github_repo_url <- decode.field(14, decode.optional(decode.string))
    use queued_at <- decode.field(15, decode.optional(pog.timestamp_decoder()))
    use worker_id <- decode.field(16, decode.optional(decode.string))
    decode.success(UpdateTaskStatusQueuedRow(
      id:,
      project_id:,
      title:,
      description:,
      acceptance_criteria:,
      status:,
      priority:,
      model_name:,
      dependencies:,
      created_at:,
      updated_at:,
      started_at:,
      completed_at:,
      is_agentic:,
      github_repo_url:,
      queued_at:,
      worker_id:,
    ))
  }

  "-- Update task status to queued
UPDATE tasks
SET status = $1, queued_at = COALESCE(queued_at::timestamp, $2), updated_at = $2
WHERE id = $3
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.timestamp(arg_2))
  |> pog.parameter(pog.text(uuid.to_string(arg_3)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Update last login time
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_user_last_login(
  db: pog.Connection,
  arg_1: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Update last login time
UPDATE users
SET last_login_at = NOW()
WHERE id = $1
"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `update_workspace` query
/// defined in `./src/database/queries/sql/update_workspace.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpdateWorkspaceRow {
  UpdateWorkspaceRow(
    id: Uuid,
    organization_id: Uuid,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Option(Bool),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Update an existing workspace
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn update_workspace(
  db: pog.Connection,
  arg_1: String,
  arg_2: String,
  arg_3: String,
  arg_4: Bool,
  arg_5: Timestamp,
  arg_6: Uuid,
  arg_7: Uuid,
) -> Result(pog.Returned(UpdateWorkspaceRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use organization_id <- decode.field(1, uuid_decoder())
    use name <- decode.field(2, decode.string)
    use slug <- decode.field(3, decode.string)
    use description <- decode.field(4, decode.optional(decode.string))
    use is_active <- decode.field(5, decode.optional(decode.bool))
    use created_at <- decode.field(6, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(7, decode.optional(pog.timestamp_decoder()))
    decode.success(UpdateWorkspaceRow(
      id:,
      organization_id:,
      name:,
      slug:,
      description:,
      is_active:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Update an existing workspace
UPDATE workspaces
SET name = $1, slug = $2, description = $3, is_active = $4, updated_at = $5
WHERE id = $6 AND organization_id = $7
RETURNING id, organization_id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.bool(arg_4))
  |> pog.parameter(pog.timestamp(arg_5))
  |> pog.parameter(pog.text(uuid.to_string(arg_6)))
  |> pog.parameter(pog.text(uuid.to_string(arg_7)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `upsert_workspace_theme` query
/// defined in `./src/database/queries/sql/upsert_workspace_theme.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type UpsertWorkspaceThemeRow {
  UpsertWorkspaceThemeRow(
    id: Uuid,
    workspace_id: Uuid,
    primary_color_light: Option(String),
    secondary_color_light: Option(String),
    primary_color_dark: Option(String),
    secondary_color_dark: Option(String),
    font_family: Option(String),
    font_size_base: Option(String),
    border_radius: Option(String),
    created_at: Option(Timestamp),
    updated_at: Option(Timestamp),
  )
}

/// Upsert theme for a workspace (create or update)
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn upsert_workspace_theme(
  db: pog.Connection,
  arg_1: Uuid,
  arg_2: String,
  arg_3: String,
  arg_4: String,
  arg_5: String,
  arg_6: String,
  arg_7: String,
  arg_8: String,
  arg_9: Timestamp,
  arg_10: Timestamp,
) -> Result(pog.Returned(UpsertWorkspaceThemeRow), pog.QueryError) {
  let decoder = {
    use id <- decode.field(0, uuid_decoder())
    use workspace_id <- decode.field(1, uuid_decoder())
    use primary_color_light <- decode.field(2, decode.optional(decode.string))
    use secondary_color_light <- decode.field(3, decode.optional(decode.string))
    use primary_color_dark <- decode.field(4, decode.optional(decode.string))
    use secondary_color_dark <- decode.field(5, decode.optional(decode.string))
    use font_family <- decode.field(6, decode.optional(decode.string))
    use font_size_base <- decode.field(7, decode.optional(decode.string))
    use border_radius <- decode.field(8, decode.optional(decode.string))
    use created_at <- decode.field(9, decode.optional(pog.timestamp_decoder()))
    use updated_at <- decode.field(10, decode.optional(pog.timestamp_decoder()))
    decode.success(UpsertWorkspaceThemeRow(
      id:,
      workspace_id:,
      primary_color_light:,
      secondary_color_light:,
      primary_color_dark:,
      secondary_color_dark:,
      font_family:,
      font_size_base:,
      border_radius:,
      created_at:,
      updated_at:,
    ))
  }

  "-- Upsert theme for a workspace (create or update)
INSERT INTO workspace_themes (
  workspace_id, primary_color_light, secondary_color_light,
  primary_color_dark, secondary_color_dark, font_family, font_size_base,
  border_radius, created_at, updated_at
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
ON CONFLICT (workspace_id) DO UPDATE SET
  primary_color_light = EXCLUDED.primary_color_light,
  secondary_color_light = EXCLUDED.secondary_color_light,
  primary_color_dark = EXCLUDED.primary_color_dark,
  secondary_color_dark = EXCLUDED.secondary_color_dark,
  font_family = EXCLUDED.font_family,
  font_size_base = EXCLUDED.font_size_base,
  border_radius = EXCLUDED.border_radius,
  updated_at = EXCLUDED.updated_at
RETURNING id, workspace_id, primary_color_light, secondary_color_light,
          primary_color_dark, secondary_color_dark, font_family, font_size_base,
          border_radius, created_at::timestamp, updated_at::timestamp"
  |> pog.query
  |> pog.parameter(pog.text(uuid.to_string(arg_1)))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.text(arg_3))
  |> pog.parameter(pog.text(arg_4))
  |> pog.parameter(pog.text(arg_5))
  |> pog.parameter(pog.text(arg_6))
  |> pog.parameter(pog.text(arg_7))
  |> pog.parameter(pog.text(arg_8))
  |> pog.parameter(pog.timestamp(arg_9))
  |> pog.parameter(pog.timestamp(arg_10))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// A row you get from running the `validate_refresh_token` query
/// defined in `./src/database/queries/sql/validate_refresh_token.sql`.
///
/// > 🐿️ This type definition was generated automatically using v4.6.0 of the
/// > [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub type ValidateRefreshTokenRow {
  ValidateRefreshTokenRow(user_id: Uuid)
}

/// Validate a refresh token and get user_id
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn validate_refresh_token(
  db: pog.Connection,
  arg_1: String,
) -> Result(pog.Returned(ValidateRefreshTokenRow), pog.QueryError) {
  let decoder = {
    use user_id <- decode.field(0, uuid_decoder())
    decode.success(ValidateRefreshTokenRow(user_id:))
  }

  "-- Validate a refresh token and get user_id
SELECT user_id
FROM refresh_tokens
WHERE token_hash = $1
  AND expires_at > NOW()
  AND revoked_at IS NULL
"
  |> pog.query
  |> pog.parameter(pog.text(arg_1))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

/// Verify a source connection and update status
///
/// > 🐿️ This function was generated automatically using v4.6.0 of
/// > the [squirrel package](https://github.com/giacomocavalieri/squirrel).
///
pub fn verify_source(
  db: pog.Connection,
  arg_1: Timestamp,
  arg_2: String,
  arg_3: Timestamp,
  arg_4: Uuid,
) -> Result(pog.Returned(Nil), pog.QueryError) {
  let decoder = decode.map(decode.dynamic, fn(_) { Nil })

  "-- Verify a source connection and update status
UPDATE sources
SET last_verified_at = $1, last_error = $2, updated_at = $3
WHERE id = $4
"
  |> pog.query
  |> pog.parameter(pog.timestamp(arg_1))
  |> pog.parameter(pog.text(arg_2))
  |> pog.parameter(pog.timestamp(arg_3))
  |> pog.parameter(pog.text(uuid.to_string(arg_4)))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

// --- Encoding/decoding utils -------------------------------------------------

/// A decoder to decode `Uuid`s coming from a Postgres query.
///
fn uuid_decoder() {
  use bit_array <- decode.then(decode.bit_array)
  case uuid.from_bit_array(bit_array) {
    Ok(uuid) -> decode.success(uuid)
    Error(_) -> decode.failure(uuid.v7(), "Uuid")
  }
}
