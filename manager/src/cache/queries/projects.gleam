/// Cached projects queries - wraps database queries with Valkey caching
import cache/connection as cache
import config
import database/connection.{type Connection}
import database/queries/projects as db_projects
import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}
import models/project.{
  type CreateProjectRequest, type Project, type ProjectStatus,
  type UpdateProjectRequest, Project,
}

const entity_type = "project"

// =============================================================================
// Cached Project Queries
// =============================================================================

/// List all projects with caching
pub fn list_projects(
  db: Connection,
  cache_client: cache.CacheConnection,
  status_filter: Option(ProjectStatus),
) -> Result(List(Project), String) {
  let cache_key = case status_filter {
    None -> cache.list_key(entity_type)
    Some(status) ->
      cache.filtered_list_key(
        entity_type,
        "status:" <> project.status_to_string(status),
      )
  }
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.list(project_decoder())) {
        Ok(projects) -> Ok(projects)
        Error(_) ->
          fetch_and_cache_projects(
            db,
            cache_client,
            cache_key,
            ttl,
            status_filter,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_projects(db, cache_client, cache_key, ttl, status_filter)
    Error(_) -> db_projects.list_projects(db, status_filter)
  }
}

fn fetch_and_cache_projects(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  status_filter: Option(ProjectStatus),
) -> Result(List(Project), String) {
  case db_projects.list_projects(db, status_filter) {
    Ok(projects) -> {
      let json_str = json.to_string(json.array(projects, project_to_json))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(projects)
    }
    Error(err) -> Error(err)
  }
}

/// Get a single project by ID with caching
pub fn get_project(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Option(Project), String) {
  let cache_key = cache.entity_key(entity_type, id)
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, project_decoder()) {
        Ok(proj) -> Ok(Some(proj))
        Error(_) ->
          fetch_and_cache_project(db, cache_client, cache_key, ttl, id)
      }
    }
    Ok(None) -> fetch_and_cache_project(db, cache_client, cache_key, ttl, id)
    Error(_) -> db_projects.get_project(db, id)
  }
}

fn fetch_and_cache_project(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  id: String,
) -> Result(Option(Project), String) {
  case db_projects.get_project(db, id) {
    Ok(Some(proj)) -> {
      let json_str = json.to_string(project_to_json(proj))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(proj))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Write Operations (with cache invalidation)
// =============================================================================

/// Create a new project
pub fn create_project(
  db: Connection,
  cache_client: cache.CacheConnection,
  req: CreateProjectRequest,
) -> Result(Project, String) {
  case db_projects.create_project(db, req) {
    Ok(proj) -> {
      invalidate_project_cache(cache_client)
      Ok(proj)
    }
    Error(err) -> Error(err)
  }
}

/// Update an existing project
pub fn update_project(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
  req: UpdateProjectRequest,
) -> Result(Option(Project), String) {
  case db_projects.update_project(db, id, req) {
    Ok(result) -> {
      invalidate_project_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Delete a project
pub fn delete_project(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Bool, String) {
  case db_projects.delete_project(db, id) {
    Ok(result) -> {
      invalidate_project_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Link a GitHub repository to a project
pub fn link_github(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
  repo_url: String,
) -> Result(Option(Project), String) {
  case db_projects.link_github(db, id, repo_url) {
    Ok(result) -> {
      invalidate_project_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Unlink GitHub repository from a project
pub fn unlink_github(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Option(Project), String) {
  case db_projects.unlink_github(db, id) {
    Ok(result) -> {
      invalidate_project_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Cache Invalidation
// =============================================================================

fn invalidate_project_cache(cache_client: cache.CacheConnection) -> Nil {
  let _ =
    cache.delete_pattern(cache_client, cache.invalidation_pattern(entity_type))
  Nil
}

fn invalidate_project_by_id(
  cache_client: cache.CacheConnection,
  id: String,
) -> Nil {
  let _ = cache.delete(cache_client, cache.entity_key(entity_type, id))
  invalidate_project_cache(cache_client)
}

// =============================================================================
// JSON Serialization
// =============================================================================

fn project_to_json(p: Project) -> json.Json {
  json.object([
    #("id", json.string(p.id)),
    #("name", json.string(p.name)),
    #("description", json.nullable(p.description, json.string)),
    #("status", json.string(project.status_to_string(p.status))),
    #("github_repo_url", json.nullable(p.github_repo_url, json.string)),
    #("created_at", json.string(p.created_at)),
    #("updated_at", json.string(p.updated_at)),
  ])
}

fn project_decoder() -> decode.Decoder(Project) {
  use id <- decode.field("id", decode.string)
  use name <- decode.field("name", decode.string)
  use description <- decode.field("description", decode.optional(decode.string))
  use status_str <- decode.field("status", decode.string)
  use github_repo_url <- decode.field(
    "github_repo_url",
    decode.optional(decode.string),
  )
  use created_at <- decode.field("created_at", decode.string)
  use updated_at <- decode.field("updated_at", decode.string)
  let status = case project.status_from_string(status_str) {
    Ok(s) -> s
    Error(_) -> project.Active
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
