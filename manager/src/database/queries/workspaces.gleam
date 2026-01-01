import birl
import database/connection.{type Connection, query_error_to_string}
import gleam/dynamic/decode
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/workspace.{
  type CreateWorkspaceRequest, type UpdateWorkspaceRequest, type Workspace,
  Workspace,
}
import pog

// =============================================================================
// Workspace Queries
// =============================================================================

/// List all workspaces for an organization, optionally filtered by active status
pub fn list_workspaces(
  db: Connection,
  organization_id: String,
  active_only: Bool,
) -> Result(List(Workspace), String) {
  let sql = case active_only {
    True ->
      "SELECT id, organization_id, name, slug, description, is_active, created_at, updated_at
       FROM workspaces WHERE organization_id = $1 AND is_active = true ORDER BY name ASC"
    False ->
      "SELECT id, organization_id, name, slug, description, is_active, created_at, updated_at
       FROM workspaces WHERE organization_id = $1 ORDER BY name ASC"
  }

  pog.query(sql)
  |> pog.parameter(pog.text(organization_id))
  |> pog.returning(workspace_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.rows })
  |> result.map_error(query_error_to_string)
}

/// Get a single workspace by ID (verify it belongs to the organization)
pub fn get_workspace(
  db: Connection,
  organization_id: String,
  workspace_id: String,
) -> Result(Option(Workspace), String) {
  let sql =
    "SELECT id, organization_id, name, slug, description, is_active, created_at, updated_at
     FROM workspaces WHERE id = $1 AND organization_id = $2"

  pog.query(sql)
  |> pog.parameter(pog.text(workspace_id))
  |> pog.parameter(pog.text(organization_id))
  |> pog.returning(workspace_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Get a single workspace by slug within an organization
pub fn get_workspace_by_slug(
  db: Connection,
  organization_id: String,
  slug: String,
) -> Result(Option(Workspace), String) {
  let sql =
    "SELECT id, organization_id, name, slug, description, is_active, created_at, updated_at
     FROM workspaces WHERE organization_id = $1 AND slug = $2"

  pog.query(sql)
  |> pog.parameter(pog.text(organization_id))
  |> pog.parameter(pog.text(slug))
  |> pog.returning(workspace_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Create a new workspace within an organization
pub fn create_workspace(
  db: Connection,
  organization_id: String,
  req: CreateWorkspaceRequest,
) -> Result(Workspace, String) {
  let now = birl.to_iso8601(birl.now())

  let sql =
    "INSERT INTO workspaces (organization_id, name, slug, description, created_at, updated_at)
     VALUES ($1, $2, $3, $4, $5, $6)
     RETURNING id, organization_id, name, slug, description, is_active, created_at, updated_at"

  pog.query(sql)
  |> pog.parameter(pog.text(organization_id))
  |> pog.parameter(pog.text(req.name))
  |> pog.parameter(pog.text(req.slug))
  |> pog.parameter(pog.nullable(pog.text, req.description))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(now))
  |> pog.returning(workspace_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(ws) -> ws
      Error(_) -> panic as "Insert should return a row"
    }
  })
  |> result.map_error(query_error_to_string)
}

/// Update an existing workspace
pub fn update_workspace(
  db: Connection,
  organization_id: String,
  workspace_id: String,
  req: UpdateWorkspaceRequest,
) -> Result(Option(Workspace), String) {
  // First get the existing workspace
  case get_workspace(db, organization_id, workspace_id) {
    Ok(Some(existing)) -> {
      let now = birl.to_iso8601(birl.now())
      let name = option.unwrap(req.name, existing.name)
      let slug = option.unwrap(req.slug, existing.slug)
      let description = case req.description {
        Some(d) -> Some(d)
        None -> existing.description
      }
      let is_active = option.unwrap(req.is_active, existing.is_active)

      let sql =
        "UPDATE workspaces SET name = $1, slug = $2, description = $3,
         is_active = $4, updated_at = $5
         WHERE id = $6 AND organization_id = $7
         RETURNING id, organization_id, name, slug, description, is_active, created_at, updated_at"

      pog.query(sql)
      |> pog.parameter(pog.text(name))
      |> pog.parameter(pog.text(slug))
      |> pog.parameter(pog.nullable(pog.text, description))
      |> pog.parameter(pog.bool(is_active))
      |> pog.parameter(pog.text(now))
      |> pog.parameter(pog.text(workspace_id))
      |> pog.parameter(pog.text(organization_id))
      |> pog.returning(workspace_row_decoder())
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

/// Delete a workspace by ID
pub fn delete_workspace(
  db: Connection,
  organization_id: String,
  workspace_id: String,
) -> Result(Bool, String) {
  let sql = "DELETE FROM workspaces WHERE id = $1 AND organization_id = $2"

  pog.query(sql)
  |> pog.parameter(pog.text(workspace_id))
  |> pog.parameter(pog.text(organization_id))
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.count > 0 })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Decoders
// =============================================================================

fn workspace_row_decoder() -> decode.Decoder(Workspace) {
  use id <- decode.field(0, decode.string)
  use organization_id <- decode.field(1, decode.string)
  use name <- decode.field(2, decode.string)
  use slug <- decode.field(3, decode.string)
  use description <- decode.field(4, decode.optional(decode.string))
  use is_active <- decode.field(5, decode.bool)
  use created_at <- decode.field(6, decode.string)
  use updated_at <- decode.field(7, decode.string)

  decode.success(Workspace(
    id: id,
    organization_id: organization_id,
    name: name,
    slug: slug,
    description: description,
    is_active: is_active,
    created_at: created_at,
    updated_at: updated_at,
  ))
}
