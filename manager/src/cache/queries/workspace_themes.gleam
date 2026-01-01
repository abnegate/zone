/// Cached workspace theme queries - wraps database queries with Valkey caching
import cache/connection as cache
import config
import database/connection.{type Connection}
import database/queries/workspace_themes as db_themes
import gleam/json
import gleam/option.{type Option, None, Some}
import models/workspace_theme.{
  type UpdateWorkspaceThemeRequest, type WorkspaceTheme,
}

const entity_type = "workspace_theme"

// =============================================================================
// Cached Theme Queries
// =============================================================================

/// Get theme for a workspace with caching
pub fn get_theme(
  db: Connection,
  cache_client: cache.CacheConnection,
  workspace_id: String,
) -> Result(Option(WorkspaceTheme), String) {
  let cache_key = cache.entity_key(entity_type, workspace_id)
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, workspace_theme.decoder()) {
        Ok(theme) -> Ok(Some(theme))
        Error(_) ->
          fetch_and_cache_theme(db, cache_client, cache_key, ttl, workspace_id)
      }
    }
    Ok(None) ->
      fetch_and_cache_theme(db, cache_client, cache_key, ttl, workspace_id)
    Error(_) -> db_themes.get_theme(db, workspace_id)
  }
}

fn fetch_and_cache_theme(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  workspace_id: String,
) -> Result(Option(WorkspaceTheme), String) {
  case db_themes.get_theme(db, workspace_id) {
    Ok(Some(theme)) -> {
      let json_str = json.to_string(workspace_theme.to_json(theme))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(theme))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Write Operations (with cache invalidation)
// =============================================================================

/// Upsert theme for a workspace
pub fn upsert_theme(
  db: Connection,
  cache_client: cache.CacheConnection,
  workspace_id: String,
  req: UpdateWorkspaceThemeRequest,
) -> Result(WorkspaceTheme, String) {
  case db_themes.upsert_theme(db, workspace_id, req) {
    Ok(theme) -> {
      invalidate_theme_cache(cache_client, workspace_id)
      Ok(theme)
    }
    Error(err) -> Error(err)
  }
}

/// Delete theme for a workspace (reset to defaults)
pub fn delete_theme(
  db: Connection,
  cache_client: cache.CacheConnection,
  workspace_id: String,
) -> Result(Bool, String) {
  case db_themes.delete_theme(db, workspace_id) {
    Ok(result) -> {
      invalidate_theme_cache(cache_client, workspace_id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Cache Invalidation
// =============================================================================

fn invalidate_theme_cache(
  cache_client: cache.CacheConnection,
  workspace_id: String,
) -> Nil {
  let _ =
    cache.delete(cache_client, cache.entity_key(entity_type, workspace_id))
  Nil
}
