import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/time/duration
import gleam/time/timestamp.{type Timestamp}
import models/organization.{
  type CreateOrganizationRequest, type Organization,
  type UpdateOrganizationRequest, Organization,
}
import youid/uuid

// =============================================================================
// Organization Queries (using Squirrel-generated SQL)
// =============================================================================

/// List all organizations, optionally filtered by active status
pub fn list_organizations(
  db: Connection,
  active_only: Bool,
) -> Result(List(Organization), String) {
  case active_only {
    True ->
      sql.list_organizations_active(db)
      |> result.map(fn(returned) {
        list.map(returned.rows, row_to_organization_active)
      })
      |> result.map_error(query_error_to_string)
    False ->
      sql.list_organizations_all(db)
      |> result.map(fn(returned) {
        list.map(returned.rows, row_to_organization_all)
      })
      |> result.map_error(query_error_to_string)
  }
}

/// Get a single organization by ID
pub fn get_organization(
  db: Connection,
  id: String,
) -> Result(Option(Organization), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      sql.get_organization_by_id(db, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(row_to_organization_get)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Get a single organization by slug
pub fn get_organization_by_slug(
  db: Connection,
  slug: String,
) -> Result(Option(Organization), String) {
  sql.get_organization_by_slug(db, slug)
  |> result.map(fn(returned) {
    list.first(returned.rows)
    |> result.map(row_to_organization_slug)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Create a new organization
pub fn create_organization(
  db: Connection,
  req: CreateOrganizationRequest,
) -> Result(Organization, String) {
  let now = timestamp.system_time()
  let description = option.unwrap(req.description, "")

  sql.create_organization(db, req.name, req.slug, description, now, now)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(row) -> row_to_organization_create(row)
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
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      case get_organization(db, id) {
        Ok(Some(existing)) -> {
          let now = timestamp.system_time()
          let name = option.unwrap(req.name, existing.name)
          let slug = option.unwrap(req.slug, existing.slug)
          let description = case req.description {
            Some(d) -> d
            None -> option.unwrap(existing.description, "")
          }
          let is_active = option.unwrap(req.is_active, existing.is_active)

          sql.update_organization(
            db,
            name,
            slug,
            description,
            is_active,
            now,
            uuid_id,
          )
          |> result.map(fn(returned) {
            list.first(returned.rows)
            |> result.map(row_to_organization_update)
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

/// Delete an organization by ID
pub fn delete_organization(db: Connection, id: String) -> Result(Bool, String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      sql.delete_organization(db, uuid_id)
      |> result.map(fn(returned) { returned.count > 0 })
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

fn bool_option_to_bool(opt: Option(Bool)) -> Bool {
  option.unwrap(opt, True)
}

fn empty_string_to_none(opt: Option(String)) -> Option(String) {
  case opt {
    Some("") -> None
    other -> other
  }
}

fn row_to_organization_all(row: sql.ListOrganizationsAllRow) -> Organization {
  Organization(
    id: uuid.to_string(row.id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_organization_active(
  row: sql.ListOrganizationsActiveRow,
) -> Organization {
  Organization(
    id: uuid.to_string(row.id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_organization_get(row: sql.GetOrganizationByIdRow) -> Organization {
  Organization(
    id: uuid.to_string(row.id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_organization_slug(row: sql.GetOrganizationBySlugRow) -> Organization {
  Organization(
    id: uuid.to_string(row.id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_organization_create(row: sql.CreateOrganizationRow) -> Organization {
  Organization(
    id: uuid.to_string(row.id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn row_to_organization_update(row: sql.UpdateOrganizationRow) -> Organization {
  Organization(
    id: uuid.to_string(row.id),
    name: row.name,
    slug: row.slug,
    description: empty_string_to_none(row.description),
    is_active: bool_option_to_bool(row.is_active),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}
