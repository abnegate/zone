import birl
import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/workspace.{
  type CreateWorkspaceRequest, type UpdateWorkspaceRequest, type Workspace,
  Workspace,
}

// =============================================================================
// Workspace Queries (using Squirrel-generated SQL)
// =============================================================================

/// List all workspaces for an organization, optionally filtered by active status
pub fn list_workspaces(
  db: Connection,
  organization_id: String,
  active_only: Bool,
) -> Result(List(Workspace), String) {
  let result = case active_only {
    True -> sql.list_workspaces_active(db, organization_id)
    False -> sql.list_workspaces_all(db, organization_id)
  }

  result
  |> result.map(fn(rows) { list.map(rows, row_to_workspace) })
  |> result.map_error(query_error_to_string)
}

/// Get a single workspace by ID (verify it belongs to the organization)
pub fn get_workspace(
  db: Connection,
  organization_id: String,
  workspace_id: String,
) -> Result(Option(Workspace), String) {
  sql.get_workspace_by_id(db, workspace_id, organization_id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_workspace)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Get a single workspace by slug within an organization
pub fn get_workspace_by_slug(
  db: Connection,
  organization_id: String,
  slug: String,
) -> Result(Option(Workspace), String) {
  sql.get_workspace_by_slug(db, organization_id, slug)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_workspace)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Create a new workspace within an organization
pub fn create_workspace(
  db: Connection,
  organization_id: String,
  req: CreateWorkspaceRequest,
) -> Result(Workspace, String) {
  let now = birl.to_iso8601(birl.now())

  sql.create_workspace(
    db,
    organization_id,
    req.name,
    req.slug,
    req.description,
    now,
    now,
  )
  |> result.map(fn(rows) {
    case list.first(rows) {
      Ok(row) -> row_to_workspace(row)
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

      sql.update_workspace(
        db,
        name,
        slug,
        description,
        is_active,
        now,
        workspace_id,
        organization_id,
      )
      |> result.map(fn(rows) {
        list.first(rows)
        |> result.map(row_to_workspace)
        |> option.from_result
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
  sql.delete_workspace(db, workspace_id, organization_id)
  |> result.map(fn(count) { count > 0 })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Mapping
// =============================================================================

fn row_to_workspace(row: sql.ListWorkspacesAllRow) -> Workspace {
  Workspace(
    id: row.id,
    organization_id: row.organization_id,
    name: row.name,
    slug: row.slug,
    description: row.description,
    is_active: row.is_active,
    created_at: row.created_at,
    updated_at: row.updated_at,
  )
}
