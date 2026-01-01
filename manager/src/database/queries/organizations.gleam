import birl
import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/organization.{
  type CreateOrganizationRequest, type Organization,
  type UpdateOrganizationRequest, Organization,
}

// =============================================================================
// Organization Queries (using Squirrel-generated SQL)
// =============================================================================

/// List all organizations, optionally filtered by active status
pub fn list_organizations(
  db: Connection,
  active_only: Bool,
) -> Result(List(Organization), String) {
  let result = case active_only {
    True -> sql.list_organizations_active(db)
    False -> sql.list_organizations_all(db)
  }

  result
  |> result.map(fn(rows) { list.map(rows, row_to_organization) })
  |> result.map_error(query_error_to_string)
}

/// Get a single organization by ID
pub fn get_organization(
  db: Connection,
  id: String,
) -> Result(Option(Organization), String) {
  sql.get_organization_by_id(db, id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_organization)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Get a single organization by slug
pub fn get_organization_by_slug(
  db: Connection,
  slug: String,
) -> Result(Option(Organization), String) {
  sql.get_organization_by_slug(db, slug)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(row_to_organization)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Create a new organization
pub fn create_organization(
  db: Connection,
  req: CreateOrganizationRequest,
) -> Result(Organization, String) {
  let now = birl.to_iso8601(birl.now())

  sql.create_organization(db, req.name, req.slug, req.description, now, now)
  |> result.map(fn(rows) {
    case list.first(rows) {
      Ok(row) -> row_to_organization(row)
      Error(_) -> panic as "Insert should return a row"
    }
  })
  |> result.map_error(query_error_to_string)
}

/// Update an existing organization
pub fn update_organization(
  db: Connection,
  id: String,
  req: UpdateOrganizationRequest,
) -> Result(Option(Organization), String) {
  // First get the existing organization
  case get_organization(db, id) {
    Ok(Some(existing)) -> {
      let now = birl.to_iso8601(birl.now())
      let name = option.unwrap(req.name, existing.name)
      let slug = option.unwrap(req.slug, existing.slug)
      let description = case req.description {
        Some(d) -> Some(d)
        None -> existing.description
      }
      let is_active = option.unwrap(req.is_active, existing.is_active)

      sql.update_organization(db, name, slug, description, is_active, now, id)
      |> result.map(fn(rows) {
        list.first(rows)
        |> result.map(row_to_organization)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// Delete an organization by ID
pub fn delete_organization(db: Connection, id: String) -> Result(Bool, String) {
  sql.delete_organization(db, id)
  |> result.map(fn(count) { count > 0 })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Mapping
// =============================================================================

fn row_to_organization(row: sql.ListOrganizationsAllRow) -> Organization {
  Organization(
    id: row.id,
    name: row.name,
    slug: row.slug,
    description: row.description,
    is_active: row.is_active,
    created_at: row.created_at,
    updated_at: row.updated_at,
  )
}
