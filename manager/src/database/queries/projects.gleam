import birl
import database/connection.{type Connection, query_error_to_string}
import gleam/dynamic/decode
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/project.{
  type CreateProjectRequest, type Project, type ProjectStatus,
  type UpdateProjectRequest, Active, Project,
}
import pog

// =============================================================================
// Project Queries
// =============================================================================

/// List all projects, optionally filtered by status
pub fn list_projects(
  db: Connection,
  status_filter: Option(ProjectStatus),
) -> Result(List(Project), String) {
  case status_filter {
    None -> {
      let sql =
        "SELECT id, name, description, status, github_repo_url, created_at, updated_at
         FROM projects ORDER BY updated_at DESC"

      pog.query(sql)
      |> pog.returning(project_row_decoder())
      |> pog.execute(db)
      |> result.map(fn(returned) { returned.rows })
      |> result.map_error(query_error_to_string)
    }
    Some(status) -> {
      let status_str = project.status_to_string(status)
      let sql =
        "SELECT id, name, description, status, github_repo_url, created_at, updated_at
         FROM projects WHERE status = $1 ORDER BY updated_at DESC"

      pog.query(sql)
      |> pog.parameter(pog.text(status_str))
      |> pog.returning(project_row_decoder())
      |> pog.execute(db)
      |> result.map(fn(returned) { returned.rows })
      |> result.map_error(query_error_to_string)
    }
  }
}

/// Get a single project by ID
pub fn get_project(
  db: Connection,
  id: String,
) -> Result(Option(Project), String) {
  let sql =
    "SELECT id, name, description, status, github_repo_url, created_at, updated_at
     FROM projects WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.returning(project_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Create a new project
pub fn create_project(
  db: Connection,
  req: CreateProjectRequest,
) -> Result(Project, String) {
  let now = birl.to_iso8601(birl.now())
  let status_str = case req.status {
    Some(status) -> project.status_to_string(status)
    None -> "active"
  }

  let sql =
    "INSERT INTO projects (name, description, status, github_repo_url, created_at, updated_at)
     VALUES ($1, $2, $3, $4, $5, $6)
     RETURNING id, name, description, status, github_repo_url, created_at, updated_at"

  pog.query(sql)
  |> pog.parameter(pog.text(req.name))
  |> pog.parameter(pog.nullable(pog.text, req.description))
  |> pog.parameter(pog.text(status_str))
  |> pog.parameter(pog.nullable(pog.text, req.github_repo_url))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(now))
  |> pog.returning(project_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(proj) -> proj
      Error(_) -> panic as "Insert should return a row"
    }
  })
  |> result.map_error(query_error_to_string)
}

/// Update an existing project
pub fn update_project(
  db: Connection,
  id: String,
  req: UpdateProjectRequest,
) -> Result(Option(Project), String) {
  // First get the existing project
  case get_project(db, id) {
    Ok(Some(existing)) -> {
      let now = birl.to_iso8601(birl.now())
      let name = option.unwrap(req.name, existing.name)
      let description = case req.description {
        Some(d) -> Some(d)
        None -> existing.description
      }
      let status = case req.status {
        Some(s) -> project.status_to_string(s)
        None -> project.status_to_string(existing.status)
      }
      let github_repo_url = case req.github_repo_url {
        Some(url) -> Some(url)
        None -> existing.github_repo_url
      }

      let sql =
        "UPDATE projects SET name = $1, description = $2, status = $3,
         github_repo_url = $4, updated_at = $5
         WHERE id = $6
         RETURNING id, name, description, status, github_repo_url, created_at, updated_at"

      pog.query(sql)
      |> pog.parameter(pog.text(name))
      |> pog.parameter(pog.nullable(pog.text, description))
      |> pog.parameter(pog.text(status))
      |> pog.parameter(pog.nullable(pog.text, github_repo_url))
      |> pog.parameter(pog.text(now))
      |> pog.parameter(pog.text(id))
      |> pog.returning(project_row_decoder())
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

/// Delete a project by ID
pub fn delete_project(db: Connection, id: String) -> Result(Bool, String) {
  let sql = "DELETE FROM projects WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Link a GitHub repository to a project
pub fn link_github(
  db: Connection,
  id: String,
  repo_url: String,
) -> Result(Option(Project), String) {
  let now = birl.to_iso8601(birl.now())
  let sql =
    "UPDATE projects SET github_repo_url = $1, updated_at = $2
     WHERE id = $3
     RETURNING id, name, description, status, github_repo_url, created_at, updated_at"

  pog.query(sql)
  |> pog.parameter(pog.text(repo_url))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(id))
  |> pog.returning(project_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Unlink GitHub repository from a project
pub fn unlink_github(
  db: Connection,
  id: String,
) -> Result(Option(Project), String) {
  let now = birl.to_iso8601(birl.now())
  let sql =
    "UPDATE projects SET github_repo_url = NULL, updated_at = $1
     WHERE id = $2
     RETURNING id, name, description, status, github_repo_url, created_at, updated_at"

  pog.query(sql)
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(id))
  |> pog.returning(project_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Decoders
// =============================================================================

fn project_row_decoder() -> decode.Decoder(Project) {
  use id <- decode.field(0, decode.string)
  use name <- decode.field(1, decode.string)
  use description <- decode.field(2, decode.optional(decode.string))
  use status_str <- decode.field(3, decode.string)
  use github_repo_url <- decode.field(4, decode.optional(decode.string))
  use created_at <- decode.field(5, decode.string)
  use updated_at <- decode.field(6, decode.string)

  let status = case project.status_from_string(status_str) {
    Ok(s) -> s
    Error(_) -> Active
  }

  decode.success(Project(
    id: id,
    name: name,
    description: description,
    status: status,
    github_repo_url: github_repo_url,
    created_at: created_at,
    updated_at: updated_at,
  ))
}
