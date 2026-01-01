import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/time/duration
import gleam/time/timestamp.{type Timestamp}
import models/project.{
  type CreateProjectRequest, type Project, type ProjectStatus,
  type UpdateProjectRequest, Active, Project,
}
import youid/uuid

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
      |> result.map(fn(returned) {
        list.map(returned.rows, row_to_project_from_all)
      })
      |> result.map_error(query_error_to_string)
    }
    Some(status) -> {
      let status_str = project.status_to_string(status)
      sql.list_projects_by_status(db, status_str)
      |> result.map(fn(returned) {
        list.map(returned.rows, row_to_project_from_status)
      })
      |> result.map_error(query_error_to_string)
    }
  }
}

/// Get a single project by ID
pub fn get_project(
  db: Connection,
  id: String,
) -> Result(Option(Project), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      sql.get_project_by_id(db, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_project_from_get)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Create a new project
pub fn create_project(
  db: Connection,
  req: CreateProjectRequest,
) -> Result(Project, String) {
  let now = timestamp.system_time()
  let status_str = case req.status {
    Some(status) -> project.status_to_string(status)
    None -> "active"
  }
  let description = option.unwrap(req.description, "")
  let github_repo_url = option.unwrap(req.github_repo_url, "")

  sql.create_project(
    db,
    req.name,
    description,
    status_str,
    github_repo_url,
    now,
    now,
  )
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(row) -> row_to_project_from_create(row)
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
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      case get_project(db, id) {
        Ok(Some(existing)) -> {
          let now = timestamp.system_time()
          let name = option.unwrap(req.name, existing.name)
          let description = case req.description {
            Some(d) -> d
            None -> option.unwrap(existing.description, "")
          }
          let status = case req.status {
            Some(s) -> project.status_to_string(s)
            None -> project.status_to_string(existing.status)
          }
          let github_repo_url = case req.github_repo_url {
            Some(url) -> url
            None -> option.unwrap(existing.github_repo_url, "")
          }

          sql.update_project(
            db,
            name,
            description,
            status,
            github_repo_url,
            now,
            uuid_id,
          )
          |> result.map(fn(returned) {
            list.first(returned.rows)
            |> result.map(row_to_project_from_update)
            |> option.from_result
          })
          |> result.map_error(query_error_to_string)
        }
        Ok(None) -> Ok(None)
        Error(err) -> Error(err)
      }
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Delete a project by ID
pub fn delete_project(db: Connection, id: String) -> Result(Bool, String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      sql.delete_project(db, uuid_id)
      |> result.map(fn(returned) { returned.count > 0 })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Link a GitHub repository to a project
pub fn link_github(
  db: Connection,
  id: String,
  repo_url: String,
) -> Result(Option(Project), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      sql.link_project_github(db, repo_url, now, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_project_from_link)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Unlink GitHub repository from a project
pub fn unlink_github(
  db: Connection,
  id: String,
) -> Result(Option(Project), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      sql.unlink_project_github(db, now, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_project_from_unlink)
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

fn timestamp_to_string(ts: Option(Timestamp)) -> String {
  case ts {
    Some(t) -> timestamp.to_rfc3339(t, duration.seconds(0))
    None -> ""
  }
}

fn empty_string_to_none(opt: Option(String)) -> Option(String) {
  case opt {
    Some("") -> None
    other -> other
  }
}

fn row_to_project_from_all(row: sql.ListProjectsAllRow) -> Project {
  let status = case project.status_from_string(row.status) {
    Ok(s) -> s
    Error(_) -> Active
  }

  Project(
    id: uuid.to_string(row.id),
    name: row.name,
    description: empty_string_to_none(row.description),
    status: status,
    github_repo_url: empty_string_to_none(row.github_repo_url),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_project_from_status(row: sql.ListProjectsByStatusRow) -> Project {
  let status = case project.status_from_string(row.status) {
    Ok(s) -> s
    Error(_) -> Active
  }

  Project(
    id: uuid.to_string(row.id),
    name: row.name,
    description: empty_string_to_none(row.description),
    status: status,
    github_repo_url: empty_string_to_none(row.github_repo_url),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_project_from_get(row: sql.GetProjectByIdRow) -> Project {
  let status = case project.status_from_string(row.status) {
    Ok(s) -> s
    Error(_) -> Active
  }

  Project(
    id: uuid.to_string(row.id),
    name: row.name,
    description: empty_string_to_none(row.description),
    status: status,
    github_repo_url: empty_string_to_none(row.github_repo_url),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_project_from_create(row: sql.CreateProjectRow) -> Project {
  let status = case project.status_from_string(row.status) {
    Ok(s) -> s
    Error(_) -> Active
  }

  Project(
    id: uuid.to_string(row.id),
    name: row.name,
    description: empty_string_to_none(row.description),
    status: status,
    github_repo_url: empty_string_to_none(row.github_repo_url),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_project_from_update(row: sql.UpdateProjectRow) -> Project {
  let status = case project.status_from_string(row.status) {
    Ok(s) -> s
    Error(_) -> Active
  }

  Project(
    id: uuid.to_string(row.id),
    name: row.name,
    description: empty_string_to_none(row.description),
    status: status,
    github_repo_url: empty_string_to_none(row.github_repo_url),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_project_from_link(row: sql.LinkProjectGithubRow) -> Project {
  let status = case project.status_from_string(row.status) {
    Ok(s) -> s
    Error(_) -> Active
  }

  Project(
    id: uuid.to_string(row.id),
    name: row.name,
    description: empty_string_to_none(row.description),
    status: status,
    github_repo_url: empty_string_to_none(row.github_repo_url),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_project_from_unlink(row: sql.UnlinkProjectGithubRow) -> Project {
  let status = case project.status_from_string(row.status) {
    Ok(s) -> s
    Error(_) -> Active
  }

  Project(
    id: uuid.to_string(row.id),
    name: row.name,
    description: empty_string_to_none(row.description),
    status: status,
    github_repo_url: empty_string_to_none(row.github_repo_url),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}
