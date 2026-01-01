/// Workspace settings routes - handles theme configuration
///
/// Routes:
/// - GET /settings/theme - Get current theme
/// - PUT /settings/theme - Update theme
/// - DELETE /settings/theme - Reset theme to defaults
import cache/queries/workspace_themes as cached_themes
import gleam/http
import gleam/json
import gleam/option.{None, Some}
import models/workspace_theme
import web.{type Context}
import wisp.{type Request, type Response}

// =============================================================================
// Route Handler
// =============================================================================

/// Handle all /settings routes for a workspace
pub fn handle_settings_route(
  req: Request,
  workspace_id: String,
  path: List(String),
  ctx: Context,
) -> Response {
  case path {
    ["theme"] -> handle_theme(req, workspace_id, ctx)
    _ -> wisp.not_found()
  }
}

// =============================================================================
// Theme Handlers
// =============================================================================

fn handle_theme(req: Request, workspace_id: String, ctx: Context) -> Response {
  case req.method {
    http.Get -> get_theme(workspace_id, ctx)
    http.Put -> update_theme(req, workspace_id, ctx)
    http.Delete -> delete_theme(workspace_id, ctx)
    _ -> wisp.method_not_allowed([http.Get, http.Put, http.Delete])
  }
}

/// GET /settings/theme - Get current theme (returns defaults if none set)
fn get_theme(workspace_id: String, ctx: Context) -> Response {
  case cached_themes.get_theme(ctx.db, ctx.cache, workspace_id) {
    Ok(Some(theme)) ->
      web.json_success([#("theme", workspace_theme.to_json(theme))])
    Ok(None) -> {
      // Return default theme
      let default = workspace_theme.default_theme(workspace_id)
      web.json_success([#("theme", workspace_theme.to_json(default))])
    }
    Error(err) -> web.internal_error(err)
  }
}

/// PUT /settings/theme - Update theme
fn update_theme(req: Request, workspace_id: String, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case workspace_theme.decode_update_request(body) {
    Ok(update_req) -> {
      case
        cached_themes.upsert_theme(ctx.db, ctx.cache, workspace_id, update_req)
      {
        Ok(theme) ->
          web.json_success([#("theme", workspace_theme.to_json(theme))])
        Error(err) -> web.internal_error(err)
      }
    }
    Error(_) -> web.bad_request("Invalid request body")
  }
}

/// DELETE /settings/theme - Reset theme to defaults
fn delete_theme(workspace_id: String, ctx: Context) -> Response {
  case cached_themes.delete_theme(ctx.db, ctx.cache, workspace_id) {
    Ok(_) -> {
      // Return the default theme
      let default = workspace_theme.default_theme(workspace_id)
      web.json_success([
        #("message", json.string("Theme reset to defaults")),
        #("theme", workspace_theme.to_json(default)),
      ])
    }
    Error(err) -> web.internal_error(err)
  }
}
