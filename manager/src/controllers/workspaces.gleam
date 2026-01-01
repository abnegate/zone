import cache/queries/organizations as cached_orgs
import cache/queries/workspaces as cached_workspaces
import controllers/workspace_settings
import gleam/http
import gleam/json
import gleam/list
import gleam/option.{None, Some}
import models/workspace
import web.{type Context}
import wisp.{type Request, type Response}

/// Handle /api/organizations/:org_id/workspaces/... routes
pub fn handle_workspaces_route(
  req: Request,
  org_id: String,
  path: List(String),
  ctx: Context,
) -> Response {
  // Verify organization exists first
  case cached_orgs.get_organization(ctx.db, ctx.cache, org_id) {
    Ok(Some(_org)) -> route_workspaces(req, org_id, path, ctx)
    Ok(None) -> web.not_found("Organization not found")
    Error(err) -> web.internal_error(err)
  }
}

fn route_workspaces(
  req: Request,
  org_id: String,
  path: List(String),
  ctx: Context,
) -> Response {
  case path {
    // GET /api/organizations/:org_id/workspaces
    // POST /api/organizations/:org_id/workspaces
    [] -> handle_workspaces_collection(req, org_id, ctx)

    // GET/PATCH/DELETE /api/organizations/:org_id/workspaces/:ws_id
    [ws_id] -> handle_single_workspace(req, org_id, ws_id, ctx)

    // Nested resources under workspace
    // /api/organizations/:org_id/workspaces/:ws_id/projects/...
    [ws_id, "projects", ..rest] ->
      handle_nested_resource(req, org_id, ws_id, "projects", rest, ctx)

    // /api/organizations/:org_id/workspaces/:ws_id/chats/...
    [ws_id, "chats", ..rest] ->
      handle_nested_resource(req, org_id, ws_id, "chats", rest, ctx)

    // /api/organizations/:org_id/workspaces/:ws_id/tasks/...
    [ws_id, "tasks", ..rest] ->
      handle_nested_resource(req, org_id, ws_id, "tasks", rest, ctx)

    // /api/organizations/:org_id/workspaces/:ws_id/settings/...
    [ws_id, "settings", ..rest] ->
      handle_settings(req, org_id, ws_id, rest, ctx)

    _ -> wisp.not_found()
  }
}

/// Handle settings routes under a workspace
fn handle_settings(
  req: Request,
  org_id: String,
  ws_id: String,
  rest: List(String),
  ctx: Context,
) -> Response {
  // Verify workspace exists and belongs to organization
  case cached_workspaces.get_workspace(ctx.db, ctx.cache, org_id, ws_id) {
    Ok(Some(_ws)) ->
      workspace_settings.handle_settings_route(req, ws_id, rest, ctx)
    Ok(None) -> web.not_found("Workspace not found")
    Error(err) -> web.internal_error(err)
  }
}

/// Handle nested resources under a workspace
fn handle_nested_resource(
  req: Request,
  org_id: String,
  ws_id: String,
  resource_type: String,
  rest: List(String),
  ctx: Context,
) -> Response {
  // Verify workspace exists and belongs to organization
  case cached_workspaces.get_workspace(ctx.db, ctx.cache, org_id, ws_id) {
    Ok(Some(_ws)) -> {
      // Delegate to resource-specific handlers
      // These will be imported and called once the existing routes are updated
      case resource_type {
        "projects" ->
          web.json_success([
            #(
              "message",
              json.string("Projects endpoint - workspace_id: " <> ws_id),
            ),
            #("path", json.array(rest, json.string)),
          ])
        "chats" ->
          web.json_success([
            #(
              "message",
              json.string("Chats endpoint - workspace_id: " <> ws_id),
            ),
            #("path", json.array(rest, json.string)),
          ])
        "tasks" ->
          web.json_success([
            #(
              "message",
              json.string("Tasks endpoint - workspace_id: " <> ws_id),
            ),
            #("path", json.array(rest, json.string)),
          ])
        _ -> wisp.not_found()
      }
    }
    Ok(None) -> web.not_found("Workspace not found")
    Error(err) -> web.internal_error(err)
  }
}

/// Handle /api/organizations/:org_id/workspaces (collection)
fn handle_workspaces_collection(
  req: Request,
  org_id: String,
  ctx: Context,
) -> Response {
  case req.method {
    http.Get -> list_workspaces(req, org_id, ctx)
    http.Post -> create_workspace(req, org_id, ctx)
    _ -> wisp.method_not_allowed([http.Get, http.Post])
  }
}

/// Handle /api/organizations/:org_id/workspaces/:ws_id (single workspace)
fn handle_single_workspace(
  req: Request,
  org_id: String,
  ws_id: String,
  ctx: Context,
) -> Response {
  case req.method {
    http.Get -> get_workspace(org_id, ws_id, ctx)
    http.Patch -> update_workspace(req, org_id, ws_id, ctx)
    http.Delete -> delete_workspace(org_id, ws_id, ctx)
    _ -> wisp.method_not_allowed([http.Get, http.Patch, http.Delete])
  }
}

// =============================================================================
// Handlers
// =============================================================================

/// GET /api/organizations/:org_id/workspaces - List all workspaces
fn list_workspaces(req: Request, org_id: String, ctx: Context) -> Response {
  // Parse optional active_only filter from query params
  let active_only = case wisp.get_query(req) {
    query_params -> {
      case list.find(query_params, fn(p) { p.0 == "active" }) {
        Ok(#(_, "true")) -> True
        _ -> False
      }
    }
  }

  case
    cached_workspaces.list_workspaces(ctx.db, ctx.cache, org_id, active_only)
  {
    Ok(ws_list) ->
      web.json_success([#("workspaces", json.array(ws_list, workspace.to_json))])
    Error(err) -> web.internal_error(err)
  }
}

/// GET /api/organizations/:org_id/workspaces/:ws_id - Get a single workspace
fn get_workspace(org_id: String, ws_id: String, ctx: Context) -> Response {
  case cached_workspaces.get_workspace(ctx.db, ctx.cache, org_id, ws_id) {
    Ok(Some(ws)) -> web.json_success([#("workspace", workspace.to_json(ws))])
    Ok(None) -> web.not_found("Workspace not found")
    Error(err) -> web.internal_error(err)
  }
}

/// POST /api/organizations/:org_id/workspaces - Create a new workspace
fn create_workspace(req: Request, org_id: String, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case workspace.decode_create_request(body) {
    Ok(create_req) -> {
      case
        cached_workspaces.create_workspace(
          ctx.db,
          ctx.cache,
          org_id,
          create_req,
        )
      {
        Ok(ws) -> web.json_created([#("workspace", workspace.to_json(ws))])
        Error(err) -> web.internal_error(err)
      }
    }
    Error(_) -> web.bad_request("Invalid request body")
  }
}

/// PATCH /api/organizations/:org_id/workspaces/:ws_id - Update a workspace
fn update_workspace(
  req: Request,
  org_id: String,
  ws_id: String,
  ctx: Context,
) -> Response {
  use body <- wisp.require_string_body(req)

  case workspace.decode_update_request(body) {
    Ok(update_req) -> {
      case
        cached_workspaces.update_workspace(
          ctx.db,
          ctx.cache,
          org_id,
          ws_id,
          update_req,
        )
      {
        Ok(Some(ws)) ->
          web.json_success([#("workspace", workspace.to_json(ws))])
        Ok(None) -> web.not_found("Workspace not found")
        Error(err) -> web.internal_error(err)
      }
    }
    Error(_) -> web.bad_request("Invalid request body")
  }
}

/// DELETE /api/organizations/:org_id/workspaces/:ws_id - Delete a workspace
fn delete_workspace(org_id: String, ws_id: String, ctx: Context) -> Response {
  case cached_workspaces.delete_workspace(ctx.db, ctx.cache, org_id, ws_id) {
    Ok(True) -> wisp.no_content()
    Ok(False) -> web.not_found("Workspace not found")
    Error(err) -> web.internal_error(err)
  }
}
