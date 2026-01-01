import birl
import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/project.{
  type CreateProjectRequest, type Project, type ProjectStatus,
  type UpdateProjectRequest, Active, Project,
}

// =============================================================================
// Project Queries (using Squirrel-generated SQL)
// =============================================================================

/// List all projects, optionally filtered by status
pub fn list_projects(
  db: Connection,
  status_filter: Option(ProjectStatus),
) -> Result(List(Project), String) {
  case status_filter {
    None -> {
      sql.list_projects_all(db)
      |> result.map(fn(rows) { list.map(rows, row_to_project) })
      |> result.map_error(query_error_to_string)
    }
    Some(status) -> {
      let status_str = project.status_to_string(status)
      sql.list_projects_by_status(db, status_str)
      |> result.map(fn(rows) { list.map(rows, row_to_project) })
      |> result.map_error(query_error_to_string)
    }
  }
}

/// Get a single project by ID
pub fn get_project(
  db: Connection,
  id: String,
) -> Result(Option(Project), String) {
  sql.get_project_by_id(db, id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_project)
    |> option.from_result
  })
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

  sql.create_project(
    db,
    req.name,
    req.description,
    status_str,
    req.github_repo_url,
    now,
    now,
  )
  |> result.map(fn(rows) {
    case list.first(rows) {
      Ok(row) -> row_to_project(row)
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

      sql.update_project(db, name, description, status, github_repo_url, now, id)
      |> result.map(fn(rows) {
        list.first(rows)
        |> result.map(row_to_project)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// Delete a project by ID
pub fn delete_project(db: Connection, id: String) -> Result(Bool, String) {
  sql.delete_project(db, id)
  |> result.map(fn(count) { count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Link a GitHub repository to a project
pub fn link_github(
  db: Connection,
  id: String,
  repo_url: String,
) -> Result(Option(Project), String) {
  let now = birl.to_iso8601(birl.now())
  sql.link_project_github(db, repo_url, now, id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_project)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Unlink GitHub repository from a project
pub fn unlink_github(
  db: Connection,
  id: String,
) -> Result(Option(Project), String) {
  let now = birl.to_iso8601(birl.now())
  sql.unlink_project_github(db, now, id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_project)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Mapping
// =============================================================================

fn row_to_project(row: sql.ListProjectsAllRow) -> Project {
  let status = case project.status_from_string(row.status) {
    Ok(s) -> s
    Error(_) -> Active
  }

  Project(
    id: row.id,
    name: row.name,
    description: row.description,
    status: status,
    github_repo_url: row.github_repo_url,
    created_at: row.created_at,
    updated_at: row.updated_at,
  )
}
