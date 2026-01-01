import cache/queries/organizations as cached_orgs
import controllers/workspaces as workspaces_routes
import gleam/http
import gleam/json
import gleam/list
import gleam/option.{None, Some}
import models/organization
import web.{type Context}
import wisp.{type Request, type Response}

/// Handle all /api/organizations routes
pub fn handle_organizations_route(
  req: Request,
  path: List(String),
  ctx: Context,
) -> Response {
  case path {
    // GET /api/organizations - List all organizations
    // POST /api/organizations - Create organization
    [] -> handle_organizations_collection(req, ctx)

    // GET /api/organizations/:id - Get single organization
    // PATCH /api/organizations/:id - Update organization
    // DELETE /api/organizations/:id - Delete organization
    [org_id] -> handle_single_organization(req, org_id, ctx)

    // /api/organizations/:org_id/workspaces/...
    [org_id, "workspaces", ..rest] ->
      workspaces_routes.handle_workspaces_route(req, org_id, rest, ctx)

    _ -> wisp.not_found()
  }
}

/// Handle /api/organizations (collection)
fn handle_organizations_collection(req: Request, ctx: Context) -> Response {
  case req.method {
    http.Get -> list_organizations(req, ctx)
    http.Post -> create_organization(req, ctx)
    _ -> wisp.method_not_allowed([http.Get, http.Post])
  }
}

/// Handle /api/organizations/:id (single organization)
fn handle_single_organization(
  req: Request,
  id: String,
  ctx: Context,
) -> Response {
  case req.method {
    http.Get -> get_organization(id, ctx)
    http.Patch -> update_organization(req, id, ctx)
    http.Delete -> delete_organization(id, ctx)
    _ -> wisp.method_not_allowed([http.Get, http.Patch, http.Delete])
  }
}

// =============================================================================
// Handlers
// =============================================================================

/// GET /api/organizations - List all organizations
fn list_organizations(req: Request, ctx: Context) -> Response {
  // Parse optional active_only filter from query params
  let active_only = case wisp.get_query(req) {
    query_params -> {
      case list.find(query_params, fn(p) { p.0 == "active" }) {
        Ok(#(_, "true")) -> True
        _ -> False
      }
    }
  }

  case cached_orgs.list_organizations(ctx.db, ctx.cache, active_only) {
    Ok(org_list) ->
      web.json_success([
        #("organizations", json.array(org_list, organization.to_json)),
      ])
    Error(err) -> web.internal_error(err)
  }
}

/// GET /api/organizations/:id - Get a single organization
fn get_organization(id: String, ctx: Context) -> Response {
  case cached_orgs.get_organization(ctx.db, ctx.cache, id) {
    Ok(Some(org)) ->
      web.json_success([#("organization", organization.to_json(org))])
    Ok(None) -> web.not_found("Organization not found")
    Error(err) -> web.internal_error(err)
  }
}

/// POST /api/organizations - Create a new organization
fn create_organization(req: Request, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case organization.decode_create_request(body) {
    Ok(create_req) -> {
      case cached_orgs.create_organization(ctx.db, ctx.cache, create_req) {
        Ok(org) ->
          web.json_created([#("organization", organization.to_json(org))])
        Error(err) -> web.internal_error(err)
      }
    }
    Error(_) -> web.bad_request("Invalid request body")
  }
}

/// PATCH /api/organizations/:id - Update an organization
fn update_organization(req: Request, id: String, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case organization.decode_update_request(body) {
    Ok(update_req) -> {
      case cached_orgs.update_organization(ctx.db, ctx.cache, id, update_req) {
        Ok(Some(org)) ->
          web.json_success([#("organization", organization.to_json(org))])
        Ok(None) -> web.not_found("Organization not found")
        Error(err) -> web.internal_error(err)
      }
    }
    Error(_) -> web.bad_request("Invalid request body")
  }
}

/// DELETE /api/organizations/:id - Delete an organization
fn delete_organization(id: String, ctx: Context) -> Response {
  case cached_orgs.delete_organization(ctx.db, ctx.cache, id) {
    Ok(True) -> wisp.no_content()
    Ok(False) -> web.not_found("Organization not found")
    Error(err) -> web.internal_error(err)
  }
}
