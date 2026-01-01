/// Cached organizations queries - wraps database queries with Valkey caching
import cache/connection as cache
import config
import database/connection.{type Connection}
import database/queries/organizations as db_orgs
import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}
import models/organization.{
  type CreateOrganizationRequest, type Organization,
  type UpdateOrganizationRequest, Organization,
}

const entity_type = "organization"

// =============================================================================
// Cached Organization Queries
// =============================================================================

/// List all organizations with caching
pub fn list_organizations(
  db: Connection,
  cache_client: cache.CacheConnection,
  active_only: Bool,
) -> Result(List(Organization), String) {
  let cache_key = case active_only {
    True -> cache.filtered_list_key(entity_type, "active:true")
    False -> cache.list_key(entity_type)
  }
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.list(organization_decoder())) {
        Ok(orgs) -> Ok(orgs)
        Error(_) ->
          fetch_and_cache_organizations(
            db,
            cache_client,
            cache_key,
            ttl,
            active_only,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_organizations(
        db,
        cache_client,
        cache_key,
        ttl,
        active_only,
      )
    Error(_) -> db_orgs.list_organizations(db, active_only)
  }
}

fn fetch_and_cache_organizations(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  active_only: Bool,
) -> Result(List(Organization), String) {
  case db_orgs.list_organizations(db, active_only) {
    Ok(orgs) -> {
      let json_str = json.to_string(json.array(orgs, organization_to_json))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(orgs)
    }
    Error(err) -> Error(err)
  }
}

/// Get a single organization by ID with caching
pub fn get_organization(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Option(Organization), String) {
  let cache_key = cache.entity_key(entity_type, id)
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, organization_decoder()) {
        Ok(org) -> Ok(Some(org))
        Error(_) ->
          fetch_and_cache_organization(db, cache_client, cache_key, ttl, id)
      }
    }
    Ok(None) ->
      fetch_and_cache_organization(db, cache_client, cache_key, ttl, id)
    Error(_) -> db_orgs.get_organization(db, id)
  }
}

fn fetch_and_cache_organization(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  id: String,
) -> Result(Option(Organization), String) {
  case db_orgs.get_organization(db, id) {
    Ok(Some(org)) -> {
      let json_str = json.to_string(organization_to_json(org))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(org))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// Get a single organization by slug with caching
pub fn get_organization_by_slug(
  db: Connection,
  cache_client: cache.CacheConnection,
  slug: String,
) -> Result(Option(Organization), String) {
  let cache_key = cache.filtered_list_key(entity_type, "slug:" <> slug)
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, organization_decoder()) {
        Ok(org) -> Ok(Some(org))
        Error(_) ->
          fetch_and_cache_organization_by_slug(
            db,
            cache_client,
            cache_key,
            ttl,
            slug,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_organization_by_slug(
        db,
        cache_client,
        cache_key,
        ttl,
        slug,
      )
    Error(_) -> db_orgs.get_organization_by_slug(db, slug)
  }
}

fn fetch_and_cache_organization_by_slug(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  slug: String,
) -> Result(Option(Organization), String) {
  case db_orgs.get_organization_by_slug(db, slug) {
    Ok(Some(org)) -> {
      let json_str = json.to_string(organization_to_json(org))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(org))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Write Operations (with cache invalidation)
// =============================================================================

/// Create a new organization
pub fn create_organization(
  db: Connection,
  cache_client: cache.CacheConnection,
  req: CreateOrganizationRequest,
) -> Result(Organization, String) {
  case db_orgs.create_organization(db, req) {
    Ok(org) -> {
      invalidate_organization_cache(cache_client)
      Ok(org)
    }
    Error(err) -> Error(err)
  }
}

/// Update an existing organization
pub fn update_organization(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
  req: UpdateOrganizationRequest,
) -> Result(Option(Organization), String) {
  case db_orgs.update_organization(db, id, req) {
    Ok(result) -> {
      invalidate_organization_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Delete an organization
pub fn delete_organization(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Bool, String) {
  case db_orgs.delete_organization(db, id) {
    Ok(result) -> {
      invalidate_organization_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Cache Invalidation
// =============================================================================

fn invalidate_organization_cache(cache_client: cache.CacheConnection) -> Nil {
  let _ =
    cache.delete_pattern(cache_client, cache.invalidation_pattern(entity_type))
  Nil
}

fn invalidate_organization_by_id(
  cache_client: cache.CacheConnection,
  id: String,
) -> Nil {
  let _ = cache.delete(cache_client, cache.entity_key(entity_type, id))
  invalidate_organization_cache(cache_client)
}

// =============================================================================
// JSON Serialization
// =============================================================================

fn organization_to_json(org: Organization) -> json.Json {
  json.object([
    #("id", json.string(org.id)),
    #("name", json.string(org.name)),
    #("slug", json.string(org.slug)),
    #("description", json.nullable(org.description, json.string)),
    #("is_active", json.bool(org.is_active)),
    #("created_at", json.string(org.created_at)),
    #("updated_at", json.string(org.updated_at)),
  ])
}

fn organization_decoder() -> decode.Decoder(Organization) {
  use id <- decode.field("id", decode.string)
  use name <- decode.field("name", decode.string)
  use slug <- decode.field("slug", decode.string)
  use description <- decode.field("description", decode.optional(decode.string))
  use is_active <- decode.field("is_active", decode.bool)
  use created_at <- decode.field("created_at", decode.string)
  use updated_at <- decode.field("updated_at", decode.string)

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
