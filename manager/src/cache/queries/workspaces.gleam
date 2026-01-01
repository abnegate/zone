/// Cached workspaces queries - wraps database queries with Valkey caching
import cache/connection as cache
import config
import database/connection.{type Connection}
import database/queries/workspaces as db_workspaces
import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}
import models/workspace.{
  type CreateWorkspaceRequest, type UpdateWorkspaceRequest, type Workspace,
  Workspace,
}

const entity_type = "workspace"

// =============================================================================
// Cached Workspace Queries
// =============================================================================

/// List all workspaces for an organization with caching
pub fn list_workspaces(
  db: Connection,
  cache_client: cache.CacheConnection,
  organization_id: String,
  active_only: Bool,
) -> Result(List(Workspace), String) {
  let cache_key = case active_only {
    True ->
      cache.filtered_list_key(
        entity_type,
        "org:" <> organization_id <> ":active:true",
      )
    False -> cache.filtered_list_key(entity_type, "org:" <> organization_id)
  }
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.list(workspace_decoder())) {
        Ok(workspaces) -> Ok(workspaces)
        Error(_) ->
          fetch_and_cache_workspaces(
            db,
            cache_client,
            cache_key,
            ttl,
            organization_id,
            active_only,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_workspaces(
        db,
        cache_client,
        cache_key,
        ttl,
        organization_id,
        active_only,
      )
    Error(_) -> db_workspaces.list_workspaces(db, organization_id, active_only)
  }
}

fn fetch_and_cache_workspaces(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  organization_id: String,
  active_only: Bool,
) -> Result(List(Workspace), String) {
  case db_workspaces.list_workspaces(db, organization_id, active_only) {
    Ok(workspaces) -> {
      let json_str = json.to_string(json.array(workspaces, workspace_to_json))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(workspaces)
    }
    Error(err) -> Error(err)
  }
}

/// Get a single workspace by ID with caching
pub fn get_workspace(
  db: Connection,
  cache_client: cache.CacheConnection,
  organization_id: String,
  workspace_id: String,
) -> Result(Option(Workspace), String) {
  let cache_key = cache.entity_key(entity_type, workspace_id)
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, workspace_decoder()) {
        Ok(ws) -> {
          // Verify it belongs to the organization
          case ws.organization_id == organization_id {
            True -> Ok(Some(ws))
            False -> Ok(None)
          }
        }
        Error(_) ->
          fetch_and_cache_workspace(
            db,
            cache_client,
            cache_key,
            ttl,
            organization_id,
            workspace_id,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_workspace(
        db,
        cache_client,
        cache_key,
        ttl,
        organization_id,
        workspace_id,
      )
    Error(_) -> db_workspaces.get_workspace(db, organization_id, workspace_id)
  }
}

fn fetch_and_cache_workspace(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  organization_id: String,
  workspace_id: String,
) -> Result(Option(Workspace), String) {
  case db_workspaces.get_workspace(db, organization_id, workspace_id) {
    Ok(Some(ws)) -> {
      let json_str = json.to_string(workspace_to_json(ws))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(ws))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// Get a single workspace by slug with caching
pub fn get_workspace_by_slug(
  db: Connection,
  cache_client: cache.CacheConnection,
  organization_id: String,
  slug: String,
) -> Result(Option(Workspace), String) {
  let cache_key =
    cache.filtered_list_key(
      entity_type,
      "org:" <> organization_id <> ":slug:" <> slug,
    )
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, workspace_decoder()) {
        Ok(ws) -> Ok(Some(ws))
        Error(_) ->
          fetch_and_cache_workspace_by_slug(
            db,
            cache_client,
            cache_key,
            ttl,
            organization_id,
            slug,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_workspace_by_slug(
        db,
        cache_client,
        cache_key,
        ttl,
        organization_id,
        slug,
      )
    Error(_) -> db_workspaces.get_workspace_by_slug(db, organization_id, slug)
  }
}

fn fetch_and_cache_workspace_by_slug(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  organization_id: String,
  slug: String,
) -> Result(Option(Workspace), String) {
  case db_workspaces.get_workspace_by_slug(db, organization_id, slug) {
    Ok(Some(ws)) -> {
      let json_str = json.to_string(workspace_to_json(ws))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(ws))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Write Operations (with cache invalidation)
// =============================================================================

/// Create a new workspace
pub fn create_workspace(
  db: Connection,
  cache_client: cache.CacheConnection,
  organization_id: String,
  req: CreateWorkspaceRequest,
) -> Result(Workspace, String) {
  case db_workspaces.create_workspace(db, organization_id, req) {
    Ok(ws) -> {
      invalidate_workspace_cache(cache_client, organization_id)
      Ok(ws)
    }
    Error(err) -> Error(err)
  }
}

/// Update an existing workspace
pub fn update_workspace(
  db: Connection,
  cache_client: cache.CacheConnection,
  organization_id: String,
  workspace_id: String,
  req: UpdateWorkspaceRequest,
) -> Result(Option(Workspace), String) {
  case db_workspaces.update_workspace(db, organization_id, workspace_id, req) {
    Ok(result) -> {
      invalidate_workspace_by_id(cache_client, organization_id, workspace_id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Delete a workspace
pub fn delete_workspace(
  db: Connection,
  cache_client: cache.CacheConnection,
  organization_id: String,
  workspace_id: String,
) -> Result(Bool, String) {
  case db_workspaces.delete_workspace(db, organization_id, workspace_id) {
    Ok(result) -> {
      invalidate_workspace_by_id(cache_client, organization_id, workspace_id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Cache Invalidation
// =============================================================================

fn invalidate_workspace_cache(
  cache_client: cache.CacheConnection,
  organization_id: String,
) -> Nil {
  // Invalidate all workspace lists for this organization
  let _ =
    cache.delete_pattern(cache_client, cache.invalidation_pattern(entity_type))
  // Also invalidate specific org patterns
  let _ =
    cache.delete_pattern(
      cache_client,
      entity_type <> ":*:org:" <> organization_id <> "*",
    )
  Nil
}

fn invalidate_workspace_by_id(
  cache_client: cache.CacheConnection,
  organization_id: String,
  workspace_id: String,
) -> Nil {
  let _ =
    cache.delete(cache_client, cache.entity_key(entity_type, workspace_id))
  invalidate_workspace_cache(cache_client, organization_id)
}

// =============================================================================
// JSON Serialization
// =============================================================================

fn workspace_to_json(ws: Workspace) -> json.Json {
  json.object([
    #("id", json.string(ws.id)),
    #("organization_id", json.string(ws.organization_id)),
    #("name", json.string(ws.name)),
    #("slug", json.string(ws.slug)),
    #("description", json.nullable(ws.description, json.string)),
    #("is_active", json.bool(ws.is_active)),
    #("created_at", json.string(ws.created_at)),
    #("updated_at", json.string(ws.updated_at)),
  ])
}

fn workspace_decoder() -> decode.Decoder(Workspace) {
  use id <- decode.field("id", decode.string)
  use organization_id <- decode.field("organization_id", decode.string)
  use name <- decode.field("name", decode.string)
  use slug <- decode.field("slug", decode.string)
  use description <- decode.field("description", decode.optional(decode.string))
  use is_active <- decode.field("is_active", decode.bool)
  use created_at <- decode.field("created_at", decode.string)
  use updated_at <- decode.field("updated_at", decode.string)

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
