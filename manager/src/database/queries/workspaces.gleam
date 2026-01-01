import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/time/duration
import gleam/time/timestamp.{type Timestamp}
import models/workspace.{
  type CreateWorkspaceRequest, type UpdateWorkspaceRequest, type Workspace,
  Workspace,
}
import youid/uuid

// =============================================================================
// Workspace Queries (using Squirrel-generated SQL)
// =============================================================================

/// List all workspaces for an organization, optionally filtered by active status
pub fn list_workspaces(
  db: Connection,
  organization_id: String,
  active_only: Bool,
) -> Result(List(Workspace), String) {
  case uuid.from_string(organization_id) {
    Ok(org_uuid) -> {
      case active_only {
        True ->
          sql.list_workspaces_active(db, org_uuid)
          |> result.map(fn(returned) {
            list.map(returned.rows, row_to_workspace_active)
          })
          |> result.map_error(query_error_to_string)
        False ->
          sql.list_workspaces_all(db, org_uuid)
          |> result.map(fn(returned) {
            list.map(returned.rows, row_to_workspace_all)
          })
          |> result.map_error(query_error_to_string)
      }
    }
    Error(_) -> Error("Invalid organization UUID format")
  }
}

/// Get a single workspace by ID (verify it belongs to the organization)
pub fn get_workspace(
  db: Connection,
  organization_id: String,
  workspace_id: String,
) -> Result(Option(Workspace), String) {
  case uuid.from_string(workspace_id), uuid.from_string(organization_id) {
    Ok(ws_uuid), Ok(org_uuid) -> {
      sql.get_workspace_by_id(db, ws_uuid, org_uuid)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_workspace_get)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    _, _ -> Error("Invalid UUID format")
  }
}

/// Get a single workspace by slug within an organization
pub fn get_workspace_by_slug(
  db: Connection,
  organization_id: String,
  slug: String,
) -> Result(Option(Workspace), String) {
  case uuid.from_string(organization_id) {
    Ok(org_uuid) -> {
      sql.get_workspace_by_slug(db, org_uuid, slug)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_workspace_slug)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid organization UUID format")
  }
}

/// Create a new workspace within an organization
pub fn create_workspace(
  db: Connection,
  organization_id: String,
  req: CreateWorkspaceRequest,
) -> Result(Workspace, String) {
  case uuid.from_string(organization_id) {
    Ok(org_uuid) -> {
      let now = timestamp.system_time()
      let description = option.unwrap(req.description, "")

      sql.create_workspace(
        db,
        org_uuid,
        req.name,
        req.slug,
        description,
        now,
        now,
      )
      |> result.map(fn(returned) {
        case list.first(returned.rows) {
          Ok(row) -> row_to_workspace_create(row)
          Error(_) -> panic as "Insert should return a row"
        }
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid organization UUID format")
  }
}

/// Update an existing workspace
pub fn update_workspace(
  db: Connection,
  organization_id: String,
  workspace_id: String,
  req: UpdateWorkspaceRequest,
) -> Result(Option(Workspace), String) {
  case uuid.from_string(workspace_id), uuid.from_string(organization_id) {
    Ok(ws_uuid), Ok(org_uuid) -> {
      case get_workspace(db, organization_id, workspace_id) {
        Ok(Some(existing)) -> {
          let now = timestamp.system_time()
          let name = option.unwrap(req.name, existing.name)
          let slug = option.unwrap(req.slug, existing.slug)
          let description = case req.description {
            Some(d) -> d
            None -> option.unwrap(existing.description, "")
          }
          let is_active = option.unwrap(req.is_active, existing.is_active)

          sql.update_workspace(
            db,
            name,
            slug,
            description,
            is_active,
            now,
            ws_uuid,
            org_uuid,
          )
          |> result.map(fn(returned) {
            list.first(returned.rows)
            |> result.map(row_to_workspace_update)
            |> option.from_result
          })
          |> result.map_error(query_error_to_string)
        }
        Ok(None) -> Ok(None)
        Error(err) -> Error(err)
      }
    }
    _, _ -> Error("Invalid UUID format")
  }
}

/// Delete a workspace by ID
pub fn delete_workspace(
  db: Connection,
  organization_id: String,
  workspace_id: String,
) -> Result(Bool, String) {
  case uuid.from_string(workspace_id), uuid.from_string(organization_id) {
    Ok(ws_uuid), Ok(org_uuid) -> {
      sql.delete_workspace(db, ws_uuid, org_uuid)
      |> result.map(fn(returned) { returned.count > 0 })
      |> result.map_error(query_error_to_string)
    }
    _, _ -> Error("Invalid UUID format")
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

fn bool_option_to_bool(opt: Option(Bool)) -> Bool {
  option.unwrap(opt, True)
}

fn empty_string_to_none(opt: Option(String)) -> Option(String) {
  case opt {
    Some("") -> None
    other -> other
  }
}

fn row_to_workspace_all(row: sql.ListWorkspacesAllRow) -> Workspace {
  Workspace(
    id: uuid.to_string(row.id),
    organization_id: uuid.to_string(row.organization_id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_workspace_active(row: sql.ListWorkspacesActiveRow) -> Workspace {
  Workspace(
    id: uuid.to_string(row.id),
    organization_id: uuid.to_string(row.organization_id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_workspace_get(row: sql.GetWorkspaceByIdRow) -> Workspace {
  Workspace(
    id: uuid.to_string(row.id),
    organization_id: uuid.to_string(row.organization_id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_workspace_slug(row: sql.GetWorkspaceBySlugRow) -> Workspace {
  Workspace(
    id: uuid.to_string(row.id),
    organization_id: uuid.to_string(row.organization_id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_workspace_create(row: sql.CreateWorkspaceRow) -> Workspace {
  Workspace(
    id: uuid.to_string(row.id),
    organization_id: uuid.to_string(row.organization_id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_workspace_update(row: sql.UpdateWorkspaceRow) -> Workspace {
  Workspace(
    id: uuid.to_string(row.id),
    organization_id: uuid.to_string(row.organization_id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}
