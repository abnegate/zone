import birl
import database/connection.{type Connection, query_error_to_string}
import gleam/dynamic/decode
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/organization.{
  type CreateOrganizationRequest, type Organization,
  type UpdateOrganizationRequest, Organization,
}
import pog

// =============================================================================
// Organization Queries
// =============================================================================

/// List all organizations, optionally filtered by active status
pub fn list_organizations(
  db: Connection,
  active_only: Bool,
) -> Result(List(Organization), String) {
  let sql = case active_only {
    True ->
      "SELECT id, name, slug, description, is_active, created_at, updated_at
       FROM organizations WHERE is_active = true ORDER BY name ASC"
    False ->
      "SELECT id, name, slug, description, is_active, created_at, updated_at
       FROM organizations ORDER BY name ASC"
  }

  pog.query(sql)
  |> pog.returning(organization_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.rows })
  |> result.map_error(query_error_to_string)
}

/// Get a single organization by ID
pub fn get_organization(
  db: Connection,
  id: String,
) -> Result(Option(Organization), String) {
  let sql =
    "SELECT id, name, slug, description, is_active, created_at, updated_at
     FROM organizations WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.returning(organization_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Get a single organization by slug
pub fn get_organization_by_slug(
  db: Connection,
  slug: String,
) -> Result(Option(Organization), String) {
  let sql =
    "SELECT id, name, slug, description, is_active, created_at, updated_at
     FROM organizations WHERE slug = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(slug))
  |> pog.returning(organization_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Create a new organization
pub fn create_organization(
  db: Connection,
  req: CreateOrganizationRequest,
) -> Result(Organization, String) {
  let now = birl.to_iso8601(birl.now())

  let sql =
    "INSERT INTO organizations (name, slug, description, created_at, updated_at)
     VALUES ($1, $2, $3, $4, $5)
     RETURNING id, name, slug, description, is_active, created_at, updated_at"

  pog.query(sql)
  |> pog.parameter(pog.text(req.name))
  |> pog.parameter(pog.text(req.slug))
  |> pog.parameter(pog.nullable(pog.text, req.description))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(now))
  |> pog.returning(organization_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(org) -> org
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

      let sql =
        "UPDATE organizations SET name = $1, slug = $2, description = $3,
         is_active = $4, updated_at = $5
         WHERE id = $6
         RETURNING id, name, slug, description, is_active, created_at, updated_at"

      pog.query(sql)
      |> pog.parameter(pog.text(name))
      |> pog.parameter(pog.text(slug))
      |> pog.parameter(pog.nullable(pog.text, description))
      |> pog.parameter(pog.bool(is_active))
      |> pog.parameter(pog.text(now))
      |> pog.parameter(pog.text(id))
      |> pog.returning(organization_row_decoder())
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

/// Delete an organization by ID
pub fn delete_organization(db: Connection, id: String) -> Result(Bool, String) {
  let sql = "DELETE FROM organizations WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.count > 0 })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Decoders
// =============================================================================

fn organization_row_decoder() -> decode.Decoder(Organization) {
  use id <- decode.field(0, decode.string)
  use name <- decode.field(1, decode.string)
  use slug <- decode.field(2, decode.string)
  use description <- decode.field(3, decode.optional(decode.string))
  use is_active <- decode.field(4, decode.bool)
  use created_at <- decode.field(5, decode.string)
  use updated_at <- decode.field(6, decode.string)

  decode.success(Organization(
    id: id,
    name: name,
    slug: slug,
    description: description,
    is_active: is_active,
    created_at: created_at,
    updated_at: updated_at,
  ))
}
