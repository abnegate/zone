import cache/queries/projects as cached_projects
import gleam/dynamic/decode
import gleam/http
import gleam/json
import gleam/option.{None, Some}
import models/project
import web.{type Context}
import wisp.{type Request, type Response}

/// Handle all /api/projects routes
pub fn handle_projects_route(
  req: Request,
  path: List(String),
  ctx: Context,
) -> Response {
  case path {
    [] -> handle_projects_collection(req, ctx)
    [id] -> handle_single_project(req, id, ctx)
    [id, "github"] -> handle_project_github(req, id, ctx)
    _ -> wisp.not_found()
  }
}

/// Handle /api/projects (collection)
fn handle_projects_collection(req: Request, ctx: Context) -> Response {
  case req.method {
    http.Get -> list_projects(req, ctx)
    http.Post -> create_project(req, ctx)
    _ -> wisp.method_not_allowed([http.Get, http.Post])
  }
}

/// Handle /api/projects/:id (single project)
fn handle_single_project(req: Request, id: String, ctx: Context) -> Response {
  case req.method {
    http.Get -> get_project(id, ctx)
    http.Patch -> update_project(req, id, ctx)
    http.Delete -> delete_project(id, ctx)
    _ -> wisp.method_not_allowed([http.Get, http.Patch, http.Delete])
  }
}

/// Handle /api/projects/:id/github
fn handle_project_github(req: Request, id: String, ctx: Context) -> Response {
  case req.method {
    http.Put -> link_github(req, id, ctx)
    http.Delete -> unlink_github(id, ctx)
    _ -> wisp.method_not_allowed([http.Put, http.Delete])
  }
}

// =============================================================================
// Handlers
// =============================================================================

/// GET /api/projects - List all projects
fn list_projects(req: Request, ctx: Context) -> Response {
  // Parse optional status filter from query params
  let status_filter = case wisp.get_query(req) {
    [#("status", status_str), ..] -> {
      case project.status_from_string(status_str) {
        Ok(status) -> Some(status)
        Error(_) -> None
      }
    }
    _ -> None
  }

  case cached_projects.list_projects(ctx.db, ctx.cache, status_filter) {
    Ok(project_list) ->
      web.json_success([
        #("projects", json.array(project_list, project.to_json)),
      ])
    Error(err) -> web.internal_error(err)
  }
}

/// GET /api/projects/:id - Get a single project
fn get_project(id: String, ctx: Context) -> Response {
  case cached_projects.get_project(ctx.db, ctx.cache, id) {
    Ok(Some(proj)) -> web.json_success([#("project", project.to_json(proj))])
    Ok(None) -> web.not_found("Project not found")
    Error(err) -> web.internal_error(err)
  }
}

/// POST /api/projects - Create a new project
fn create_project(req: Request, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case project.decode_create_request(body) {
    Ok(create_req) -> {
      case cached_projects.create_project(ctx.db, ctx.cache, create_req) {
        Ok(proj) -> web.json_created([#("project", project.to_json(proj))])
        Error(err) -> web.internal_error(err)
      }
    }
    Error(_) -> web.bad_request("Invalid request body")
  }
}

/// PATCH /api/projects/:id - Update a project
fn update_project(req: Request, id: String, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case project.decode_update_request(body) {
    Ok(update_req) -> {
      case cached_projects.update_project(ctx.db, ctx.cache, id, update_req) {
        Ok(Some(proj)) ->
          web.json_success([#("project", project.to_json(proj))])
        Ok(None) -> web.not_found("Project not found")
        Error(err) -> web.internal_error(err)
      }
    }
    Error(_) -> web.bad_request("Invalid request body")
  }
}

/// DELETE /api/projects/:id - Delete a project
fn delete_project(id: String, ctx: Context) -> Response {
  case cached_projects.delete_project(ctx.db, ctx.cache, id) {
    Ok(True) -> wisp.no_content()
    Ok(False) -> web.not_found("Project not found")
    Error(err) -> web.internal_error(err)
  }
}

/// PUT /api/projects/:id/github - Link GitHub repo
fn link_github(req: Request, id: String, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case decode_github_link_request(body) {
    Ok(repo_url) -> {
      case cached_projects.link_github(ctx.db, ctx.cache, id, repo_url) {
        Ok(Some(proj)) ->
          web.json_success([#("project", project.to_json(proj))])
        Ok(None) -> web.not_found("Project not found")
        Error(err) -> web.internal_error(err)
      }
    }
    Error(_) ->
      web.bad_request("Invalid request body - expected {\"repo_url\": \"...\"}")
  }
}

/// DELETE /api/projects/:id/github - Unlink GitHub repo
fn unlink_github(id: String, ctx: Context) -> Response {
  case cached_projects.unlink_github(ctx.db, ctx.cache, id) {
    Ok(Some(proj)) -> web.json_success([#("project", project.to_json(proj))])
    Ok(None) -> web.not_found("Project not found")
    Error(err) -> web.internal_error(err)
  }
}

// =============================================================================
// Helpers
// =============================================================================

fn decode_github_link_request(body: String) -> Result(String, json.DecodeError) {
  let decoder = {
    use repo_url <- decode.field("repo_url", decode.string)
    decode.success(repo_url)
  }

  json.parse(body, decoder)
}
