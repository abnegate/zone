/// Cached sources queries - wraps database queries with Valkey caching
import cache/connection as cache
import config
import database/connection.{type Connection}
import database/queries/sources as db_sources
import gleam/dynamic/decode
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import models/source.{
  type CreateSourceRequest, type Source, type SourceCategory, type SourceConfig,
  type SourceType, type UpdateSourceRequest, Discord, DiscordConfig,
  DiscordSourceConfig, Filesystem, FilesystemConfig, FilesystemSourceConfig,
  GitHub, GitHubConfig, GitHubSourceConfig, GitLab, GitLabConfig,
  GitLabSourceConfig, ICal, ICalConfig, ICalSourceConfig, IMAP, IMAPConfig,
  IMAPSourceConfig, Slack, SlackConfig, SlackSourceConfig, Source, Text,
  TextConfig, TextSourceConfig, Web, WebConfig, WebSourceConfig,
}

const entity_type = "source"

// =============================================================================
// Cached Source Queries
// =============================================================================

/// List all sources with caching
pub fn list_sources(
  db: Connection,
  cache_client: cache.CacheConnection,
  type_filter: Option(SourceType),
  active_only: Bool,
) -> Result(List(Source), String) {
  let cache_key = case type_filter, active_only {
    None, False -> cache.list_key(entity_type)
    None, True -> cache.filtered_list_key(entity_type, "active:true")
    Some(t), False ->
      cache.filtered_list_key(
        entity_type,
        "type:" <> source.source_type_to_string(t),
      )
    Some(t), True ->
      cache.filtered_list_key(
        entity_type,
        "type:" <> source.source_type_to_string(t) <> ":active:true",
      )
  }
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.list(source_decoder())) {
        Ok(sources) -> Ok(sources)
        Error(_) ->
          fetch_and_cache_sources(
            db,
            cache_client,
            cache_key,
            ttl,
            type_filter,
            active_only,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_sources(
        db,
        cache_client,
        cache_key,
        ttl,
        type_filter,
        active_only,
      )
    Error(_) -> db_sources.list_sources(db, type_filter, active_only)
  }
}

fn fetch_and_cache_sources(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  type_filter: Option(SourceType),
  active_only: Bool,
) -> Result(List(Source), String) {
  case db_sources.list_sources(db, type_filter, active_only) {
    Ok(sources) -> {
      let json_str = json.to_string(json.array(sources, source_to_json))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(sources)
    }
    Error(err) -> Error(err)
  }
}

/// List sources by category with caching
pub fn list_sources_by_category(
  db: Connection,
  cache_client: cache.CacheConnection,
  category: SourceCategory,
  active_only: Bool,
) -> Result(List(Source), String) {
  let category_str = source.source_category_to_string(category)
  let cache_key = case active_only {
    True ->
      cache.filtered_list_key(
        entity_type,
        "category:" <> category_str <> ":active:true",
      )
    False -> cache.filtered_list_key(entity_type, "category:" <> category_str)
  }
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.list(source_decoder())) {
        Ok(sources) -> Ok(sources)
        Error(_) ->
          fetch_and_cache_sources_by_category(
            db,
            cache_client,
            cache_key,
            ttl,
            category,
            active_only,
          )
      }
    }
    Ok(None) ->
      fetch_and_cache_sources_by_category(
        db,
        cache_client,
        cache_key,
        ttl,
        category,
        active_only,
      )
    Error(_) -> db_sources.list_sources_by_category(db, category, active_only)
  }
}

fn fetch_and_cache_sources_by_category(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  category: SourceCategory,
  active_only: Bool,
) -> Result(List(Source), String) {
  case db_sources.list_sources_by_category(db, category, active_only) {
    Ok(sources) -> {
      let json_str = json.to_string(json.array(sources, source_to_json))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(sources)
    }
    Error(err) -> Error(err)
  }
}

/// Get a single source by ID with caching
pub fn get_source(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Option(Source), String) {
  let cache_key = cache.entity_key(entity_type, id)
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, source_decoder()) {
        Ok(src) -> Ok(Some(src))
        Error(_) -> fetch_and_cache_source(db, cache_client, cache_key, ttl, id)
      }
    }
    Ok(None) -> fetch_and_cache_source(db, cache_client, cache_key, ttl, id)
    Error(_) -> db_sources.get_source(db, id)
  }
}

fn fetch_and_cache_source(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  id: String,
) -> Result(Option(Source), String) {
  case db_sources.get_source(db, id) {
    Ok(Some(src)) -> {
      let json_str = json.to_string(source_to_json(src))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(src))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// Get source for a task with caching
pub fn get_task_source(
  db: Connection,
  cache_client: cache.CacheConnection,
  task_id: String,
) -> Result(Option(Source), String) {
  let cache_key = cache.entity_key(entity_type, "task:" <> task_id)
  let ttl = config.get_cache_ttl()

  case cache.get(cache_client, cache_key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, source_decoder()) {
        Ok(src) -> Ok(Some(src))
        Error(_) ->
          fetch_and_cache_task_source(db, cache_client, cache_key, ttl, task_id)
      }
    }
    Ok(None) ->
      fetch_and_cache_task_source(db, cache_client, cache_key, ttl, task_id)
    Error(_) -> db_sources.get_task_source(db, task_id)
  }
}

fn fetch_and_cache_task_source(
  db: Connection,
  cache_client: cache.CacheConnection,
  cache_key: String,
  ttl: Int,
  task_id: String,
) -> Result(Option(Source), String) {
  case db_sources.get_task_source(db, task_id) {
    Ok(Some(src)) -> {
      let json_str = json.to_string(source_to_json(src))
      let _ = cache.set(cache_client, cache_key, json_str, ttl)
      Ok(Some(src))
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Write Operations (with cache invalidation)
// =============================================================================

/// Create a new source
pub fn create_source(
  db: Connection,
  cache_client: cache.CacheConnection,
  req: CreateSourceRequest,
) -> Result(Source, String) {
  case db_sources.create_source(db, req) {
    Ok(src) -> {
      invalidate_source_cache(cache_client)
      Ok(src)
    }
    Error(err) -> Error(err)
  }
}

/// Update an existing source
pub fn update_source(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
  req: UpdateSourceRequest,
) -> Result(Option(Source), String) {
  case db_sources.update_source(db, id, req) {
    Ok(result) -> {
      invalidate_source_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Delete a source
pub fn delete_source(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
) -> Result(Bool, String) {
  case db_sources.delete_source(db, id) {
    Ok(result) -> {
      invalidate_source_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Verify a source
pub fn verify_source(
  db: Connection,
  cache_client: cache.CacheConnection,
  id: String,
  success: Bool,
  error_msg: Option(String),
) -> Result(Nil, String) {
  case db_sources.verify_source(db, id, success, error_msg) {
    Ok(result) -> {
      invalidate_source_by_id(cache_client, id)
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Link a source to a project
pub fn link_source_to_project(
  db: Connection,
  cache_client: cache.CacheConnection,
  project_id: String,
  source_id: String,
) -> Result(Nil, String) {
  case db_sources.link_source_to_project(db, project_id, source_id) {
    Ok(result) -> {
      // Invalidate both source and project caches
      invalidate_source_cache(cache_client)
      let _ = cache.delete_pattern(cache_client, "project:*")
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Unlink source from a project
pub fn unlink_source_from_project(
  db: Connection,
  cache_client: cache.CacheConnection,
  project_id: String,
) -> Result(Nil, String) {
  case db_sources.unlink_source_from_project(db, project_id) {
    Ok(result) -> {
      let _ = cache.delete_pattern(cache_client, "project:*")
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

/// Link a source to a task
pub fn link_source_to_task(
  db: Connection,
  cache_client: cache.CacheConnection,
  task_id: String,
  source_id: String,
) -> Result(Nil, String) {
  case db_sources.link_source_to_task(db, task_id, source_id) {
    Ok(result) -> {
      let _ =
        cache.delete(
          cache_client,
          cache.entity_key(entity_type, "task:" <> task_id),
        )
      let _ = cache.delete_pattern(cache_client, "task:*")
      Ok(result)
    }
    Error(err) -> Error(err)
  }
}

// =============================================================================
// Cache Invalidation
// =============================================================================

fn invalidate_source_cache(cache_client: cache.CacheConnection) -> Nil {
  let _ =
    cache.delete_pattern(cache_client, cache.invalidation_pattern(entity_type))
  Nil
}

fn invalidate_source_by_id(
  cache_client: cache.CacheConnection,
  id: String,
) -> Nil {
  let _ = cache.delete(cache_client, cache.entity_key(entity_type, id))
  invalidate_source_cache(cache_client)
}

// =============================================================================
// JSON Serialization
// =============================================================================

fn source_to_json(s: Source) -> json.Json {
  json.object([
    #("id", json.string(s.id)),
    #("name", json.string(s.name)),
    #("source_type", json.string(source.source_type_to_string(s.source_type))),
    #("config", config_to_json(s.config)),
    #("credentials", json.nullable(s.credentials, json.string)),
    #("description", json.nullable(s.description, json.string)),
    #("url", json.nullable(s.url, json.string)),
    #("is_active", json.bool(s.is_active)),
    #("last_verified_at", json.nullable(s.last_verified_at, json.string)),
    #("last_error", json.nullable(s.last_error, json.string)),
    #("created_at", json.string(s.created_at)),
    #("updated_at", json.string(s.updated_at)),
  ])
}

fn config_to_json(config: SourceConfig) -> json.Json {
  case config {
    GitHubSourceConfig(cfg) ->
      json.object([
        #("type", json.string("github")),
        #("owner", json.string(cfg.owner)),
        #("repo", json.string(cfg.repo)),
        #("branch", json.string(cfg.branch)),
        #("base_path", json.string(cfg.base_path)),
      ])
    GitLabSourceConfig(cfg) ->
      json.object([
        #("type", json.string("gitlab")),
        #("project_id", json.string(cfg.project_id)),
        #("host", json.string(cfg.host)),
        #("branch", json.string(cfg.branch)),
        #("base_path", json.string(cfg.base_path)),
      ])
    FilesystemSourceConfig(cfg) ->
      json.object([
        #("type", json.string("filesystem")),
        #("base_path", json.string(cfg.base_path)),
        #("allow_writes", json.bool(cfg.allow_writes)),
      ])
    ICalSourceConfig(cfg) ->
      json.object([
        #("type", json.string("ical")),
        #("url", json.string(cfg.url)),
        #("refresh_interval", json.nullable(cfg.refresh_interval, json.int)),
      ])
    IMAPSourceConfig(cfg) ->
      json.object([
        #("type", json.string("imap")),
        #("host", json.string(cfg.host)),
        #("port", json.int(cfg.port)),
        #("username", json.string(cfg.username)),
        #("use_ssl", json.bool(cfg.use_ssl)),
        #("folder", json.nullable(cfg.folder, json.string)),
      ])
    DiscordSourceConfig(cfg) ->
      json.object([
        #("type", json.string("discord")),
        #("server_id", json.string(cfg.server_id)),
        #("channel_ids", case cfg.channel_ids {
          Some(ids) -> json.array(ids, json.string)
          None -> json.null()
        }),
      ])
    SlackSourceConfig(cfg) ->
      json.object([
        #("type", json.string("slack")),
        #("workspace_id", json.string(cfg.workspace_id)),
        #("channel_ids", case cfg.channel_ids {
          Some(ids) -> json.array(ids, json.string)
          None -> json.null()
        }),
      ])
    WebSourceConfig(cfg) ->
      json.object([
        #("type", json.string("web")),
        #("url", json.string(cfg.url)),
        #("headers", case cfg.headers {
          Some(hdrs) ->
            json.object(list.map(hdrs, fn(h) { #(h.0, json.string(h.1)) }))
          None -> json.null()
        }),
      ])
    TextSourceConfig(cfg) ->
      json.object([
        #("type", json.string("text")),
        #("content", json.string(cfg.content)),
        #("label", json.nullable(cfg.label, json.string)),
      ])
  }
}

fn source_decoder() -> decode.Decoder(Source) {
  use id <- decode.field("id", decode.string)
  use name <- decode.field("name", decode.string)
  use type_str <- decode.field("source_type", decode.string)
  use config <- decode.field("config", config_decoder())
  use credentials <- decode.field("credentials", decode.optional(decode.string))
  use description <- decode.field("description", decode.optional(decode.string))
  use url <- decode.field("url", decode.optional(decode.string))
  use is_active <- decode.field("is_active", decode.bool)
  use last_verified_at <- decode.field(
    "last_verified_at",
    decode.optional(decode.string),
  )
  use last_error <- decode.field("last_error", decode.optional(decode.string))
  use created_at <- decode.field("created_at", decode.string)
  use updated_at <- decode.field("updated_at", decode.string)
  let source_type = case source.source_type_from_string(type_str) {
    Ok(t) -> t
    Error(_) -> GitHub
  }
  decode.success(Source(
    id: id,
    name: name,
    source_type: source_type,
    config: config,
    credentials: credentials,
    description: description,
    url: url,
    is_active: is_active,
    last_verified_at: last_verified_at,
    last_error: last_error,
    created_at: created_at,
    updated_at: updated_at,
  ))
}

fn config_decoder() -> decode.Decoder(SourceConfig) {
  use config_type <- decode.field("type", decode.string)
  case config_type {
    "github" -> {
      use owner <- decode.field("owner", decode.string)
      use repo <- decode.field("repo", decode.string)
      use branch <- decode.field("branch", decode.string)
      use base_path <- decode.field("base_path", decode.string)
      decode.success(
        GitHubSourceConfig(GitHubConfig(owner, repo, branch, base_path)),
      )
    }
    "gitlab" -> {
      use project_id <- decode.field("project_id", decode.string)
      use host <- decode.field("host", decode.string)
      use branch <- decode.field("branch", decode.string)
      use base_path <- decode.field("base_path", decode.string)
      decode.success(
        GitLabSourceConfig(GitLabConfig(project_id, host, branch, base_path)),
      )
    }
    "filesystem" -> {
      use base_path <- decode.field("base_path", decode.string)
      use allow_writes <- decode.field("allow_writes", decode.bool)
      decode.success(
        FilesystemSourceConfig(FilesystemConfig(base_path, allow_writes)),
      )
    }
    "ical" -> {
      use url <- decode.field("url", decode.string)
      use refresh_interval <- decode.field(
        "refresh_interval",
        decode.optional(decode.int),
      )
      decode.success(ICalSourceConfig(ICalConfig(url, refresh_interval)))
    }
    "imap" -> {
      use host <- decode.field("host", decode.string)
      use port <- decode.field("port", decode.int)
      use username <- decode.field("username", decode.string)
      use use_ssl <- decode.field("use_ssl", decode.bool)
      use folder <- decode.field("folder", decode.optional(decode.string))
      decode.success(
        IMAPSourceConfig(IMAPConfig(host, port, username, use_ssl, folder)),
      )
    }
    "discord" -> {
      use server_id <- decode.field("server_id", decode.string)
      use channel_ids <- decode.field(
        "channel_ids",
        decode.optional(decode.list(decode.string)),
      )
      decode.success(DiscordSourceConfig(DiscordConfig(server_id, channel_ids)))
    }
    "slack" -> {
      use workspace_id <- decode.field("workspace_id", decode.string)
      use channel_ids <- decode.field(
        "channel_ids",
        decode.optional(decode.list(decode.string)),
      )
      decode.success(SlackSourceConfig(SlackConfig(workspace_id, channel_ids)))
    }
    "web" -> {
      use url <- decode.field("url", decode.string)
      decode.success(WebSourceConfig(WebConfig(url, None)))
    }
    "text" -> {
      use content <- decode.field("content", decode.string)
      use label <- decode.field("label", decode.optional(decode.string))
      decode.success(TextSourceConfig(TextConfig(content, label)))
    }
    _ -> decode.success(GitHubSourceConfig(GitHubConfig("", "", "main", "")))
  }
}
