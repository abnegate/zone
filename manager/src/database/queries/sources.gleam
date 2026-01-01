import birl
import database/connection.{type Connection, query_error_to_string}
import gleam/dynamic/decode
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/source.{
  type CreateSourceRequest, type Source, type SourceCategory, type SourceConfig,
  type SourceType, type UpdateSourceRequest, Discord, DiscordConfig,
  DiscordSourceConfig, Filesystem, FilesystemConfig, FilesystemSourceConfig,
  GitHub, GitHubConfig, GitHubSourceConfig, GitLab, GitLabConfig,
  GitLabSourceConfig, ICal, ICalConfig, ICalSourceConfig, IMAP, IMAPConfig,
  IMAPSourceConfig, Slack, SlackConfig, SlackSourceConfig, Source, Text,
  TextConfig, TextSourceConfig, Web, WebConfig, WebSourceConfig,
}
import pog

// =============================================================================
// Source Queries
// =============================================================================

/// List all sources, optionally filtered by type
pub fn list_sources(
  db: Connection,
  type_filter: Option(SourceType),
  active_only: Bool,
) -> Result(List(Source), String) {
  let base_sql =
    "SELECT id, name, source_type, config, credentials_encrypted, description, url,
            is_active, last_verified_at, last_error, created_at, updated_at
     FROM sources"

  let where_clauses = []
  let where_clauses = case active_only {
    True -> ["is_active = TRUE", ..where_clauses]
    False -> where_clauses
  }
  let where_clauses = case type_filter {
    Some(t) -> [
      "source_type = '" <> source.source_type_to_string(t) <> "'",
      ..where_clauses
    ]
    None -> where_clauses
  }

  let sql = case where_clauses {
    [] -> base_sql <> " ORDER BY name ASC"
    clauses ->
      base_sql
      <> " WHERE "
      <> list.reverse(clauses) |> string.join(" AND ")
      <> " ORDER BY name ASC"
  }

  pog.query(sql)
  |> pog.returning(source_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.rows })
  |> result.map_error(query_error_to_string)
}

/// Get a single source by ID
pub fn get_source(db: Connection, id: String) -> Result(Option(Source), String) {
  let sql =
    "SELECT id, name, source_type, config, credentials_encrypted, description, url,
            is_active, last_verified_at, last_error, created_at, updated_at
     FROM sources WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.returning(source_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Get source for a task (task source or fallback to project source)
pub fn get_task_source(
  db: Connection,
  task_id: String,
) -> Result(Option(Source), String) {
  let sql =
    "SELECT s.id, s.name, s.source_type, s.config, s.credentials_encrypted, s.description, s.url,
            s.is_active, s.last_verified_at, s.last_error, s.created_at, s.updated_at
     FROM tasks t
     JOIN projects p ON p.id = t.project_id
     LEFT JOIN sources s ON s.id = COALESCE(t.source_id, p.source_id)
     WHERE t.id = $1 AND s.is_active = TRUE"

  pog.query(sql)
  |> pog.parameter(pog.text(task_id))
  |> pog.returning(source_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Create a new source
pub fn create_source(
  db: Connection,
  req: CreateSourceRequest,
) -> Result(Source, String) {
  let now = birl.to_iso8601(birl.now())
  let type_str = source.source_type_to_string(req.source_type)
  let config_json = source.config_to_json(req.config) |> json.to_string
  let url = build_source_url(req.source_type, req.config)

  let sql =
    "INSERT INTO sources (name, source_type, config, credentials_encrypted, description, url, created_at, updated_at)
     VALUES ($1, $2, $3::jsonb, $4, $5, $6, $7, $8)
     RETURNING id, name, source_type, config, credentials_encrypted, description, url,
               is_active, last_verified_at, last_error, created_at, updated_at"

  pog.query(sql)
  |> pog.parameter(pog.text(req.name))
  |> pog.parameter(pog.text(type_str))
  |> pog.parameter(pog.text(config_json))
  |> pog.parameter(pog.nullable(pog.text, req.credentials))
  |> pog.parameter(pog.nullable(pog.text, req.description))
  |> pog.parameter(pog.text(url))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(now))
  |> pog.returning(source_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(src) -> src
      Error(_) -> panic as "Insert should return a row"
    }
  })
  |> result.map_error(query_error_to_string)
}

/// Update an existing source
pub fn update_source(
  db: Connection,
  id: String,
  req: UpdateSourceRequest,
) -> Result(Option(Source), String) {
  case get_source(db, id) {
    Ok(Some(existing)) -> {
      let now = birl.to_iso8601(birl.now())
      let name = option.unwrap(req.name, existing.name)
      let config = option.unwrap(req.config, existing.config)
      let config_json = source.config_to_json(config) |> json.to_string
      let credentials = case req.credentials {
        Some(c) -> Some(c)
        None -> existing.credentials
      }
      let description = case req.description {
        Some(d) -> Some(d)
        None -> existing.description
      }
      let is_active = option.unwrap(req.is_active, existing.is_active)
      let url = build_source_url(existing.source_type, config)

      let sql =
        "UPDATE sources SET name = $1, config = $2::jsonb, credentials_encrypted = $3,
         description = $4, url = $5, is_active = $6, updated_at = $7
         WHERE id = $8
         RETURNING id, name, source_type, config, credentials_encrypted, description, url,
                   is_active, last_verified_at, last_error, created_at, updated_at"

      pog.query(sql)
      |> pog.parameter(pog.text(name))
      |> pog.parameter(pog.text(config_json))
      |> pog.parameter(pog.nullable(pog.text, credentials))
      |> pog.parameter(pog.nullable(pog.text, description))
      |> pog.parameter(pog.text(url))
      |> pog.parameter(pog.bool(is_active))
      |> pog.parameter(pog.text(now))
      |> pog.parameter(pog.text(id))
      |> pog.returning(source_row_decoder())
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

/// Delete a source by ID
pub fn delete_source(db: Connection, id: String) -> Result(Bool, String) {
  let sql = "DELETE FROM sources WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(id))
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Verify a source connection and update status
pub fn verify_source(
  db: Connection,
  id: String,
  success: Bool,
  error_msg: Option(String),
) -> Result(Nil, String) {
  let now = birl.to_iso8601(birl.now())
  let sql =
    "UPDATE sources SET last_verified_at = $1, last_error = $2, updated_at = $3
     WHERE id = $4"

  pog.query(sql)
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.nullable(pog.text, error_msg))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(id))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Link a source to a project
pub fn link_source_to_project(
  db: Connection,
  project_id: String,
  source_id: String,
) -> Result(Nil, String) {
  let now = birl.to_iso8601(birl.now())
  let sql = "UPDATE projects SET source_id = $1, updated_at = $2 WHERE id = $3"

  pog.query(sql)
  |> pog.parameter(pog.text(source_id))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(project_id))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Unlink source from a project
pub fn unlink_source_from_project(
  db: Connection,
  project_id: String,
) -> Result(Nil, String) {
  let now = birl.to_iso8601(birl.now())
  let sql =
    "UPDATE projects SET source_id = NULL, updated_at = $1 WHERE id = $2"

  pog.query(sql)
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(project_id))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Link a source to a task
pub fn link_source_to_task(
  db: Connection,
  task_id: String,
  source_id: String,
) -> Result(Nil, String) {
  let now = birl.to_iso8601(birl.now())
  let sql = "UPDATE tasks SET source_id = $1, updated_at = $2 WHERE id = $3"

  pog.query(sql)
  |> pog.parameter(pog.text(source_id))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(task_id))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Link multiple sources to a task
pub fn link_sources_to_task(
  db: Connection,
  task_id: String,
  source_ids: List(String),
) -> Result(Nil, String) {
  let now = birl.to_iso8601(birl.now())
  let ids_array = "{" <> string.join(source_ids, ",") <> "}"
  let sql =
    "UPDATE tasks SET source_ids = $1::uuid[], updated_at = $2 WHERE id = $3"

  pog.query(sql)
  |> pog.parameter(pog.text(ids_array))
  |> pog.parameter(pog.text(now))
  |> pog.parameter(pog.text(task_id))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Get all sources for a task (from source_ids array or fallback to project source)
pub fn get_task_sources(
  db: Connection,
  task_id: String,
) -> Result(List(Source), String) {
  // First try to get sources from source_ids array
  let sql_array =
    "SELECT s.id, s.name, s.source_type, s.config, s.credentials_encrypted, s.description, s.url,
            s.is_active, s.last_verified_at, s.last_error, s.created_at, s.updated_at
     FROM tasks t
     CROSS JOIN LATERAL unnest(t.source_ids) AS task_source_id
     JOIN sources s ON s.id = task_source_id
     WHERE t.id = $1 AND s.is_active = TRUE"

  let array_result =
    pog.query(sql_array)
    |> pog.parameter(pog.text(task_id))
    |> pog.returning(source_row_decoder())
    |> pog.execute(db)
    |> result.map(fn(returned) { returned.rows })
    |> result.map_error(query_error_to_string)

  case array_result {
    Ok([_, ..] as sources) -> Ok(sources)
    Ok([]) | Error(_) -> {
      // Fallback to single source_id or project source
      case get_task_source(db, task_id) {
        Ok(Some(source)) -> Ok([source])
        Ok(None) -> Ok([])
        Error(err) -> Error(err)
      }
    }
  }
}

/// Get sources for a task filtered by category
pub fn get_task_sources_by_category(
  db: Connection,
  task_id: String,
  category: SourceCategory,
) -> Result(List(Source), String) {
  case get_task_sources(db, task_id) {
    Ok(sources) -> {
      let filtered =
        sources
        |> list.filter(fn(s) {
          source.source_type_category(s.source_type) == category
        })
      Ok(filtered)
    }
    Error(err) -> Error(err)
  }
}

/// List sources by category
pub fn list_sources_by_category(
  db: Connection,
  category: SourceCategory,
  active_only: Bool,
) -> Result(List(Source), String) {
  let category_str = source.source_category_to_string(category)

  let base_sql =
    "SELECT s.id, s.name, s.source_type, s.config, s.credentials_encrypted, s.description, s.url,
            s.is_active, s.last_verified_at, s.last_error, s.created_at, s.updated_at
     FROM sources s
     JOIN source_types st ON st.name = s.source_type
     WHERE st.category = $1"

  let sql = case active_only {
    True -> base_sql <> " AND s.is_active = TRUE ORDER BY s.name ASC"
    False -> base_sql <> " ORDER BY s.name ASC"
  }

  pog.query(sql)
  |> pog.parameter(pog.text(category_str))
  |> pog.returning(source_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.rows })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Helper Functions
// =============================================================================

fn build_source_url(_source_type: SourceType, config: SourceConfig) -> String {
  case config {
    GitHubSourceConfig(cfg) ->
      "https://github.com/" <> cfg.owner <> "/" <> cfg.repo
    GitLabSourceConfig(cfg) -> cfg.host <> "/" <> cfg.project_id
    FilesystemSourceConfig(cfg) -> "file://" <> cfg.base_path
    ICalSourceConfig(cfg) -> cfg.url
    IMAPSourceConfig(cfg) -> "imap://" <> cfg.username <> "@" <> cfg.host
    DiscordSourceConfig(cfg) -> "discord://" <> cfg.server_id
    SlackSourceConfig(cfg) -> "slack://" <> cfg.workspace_id
    WebSourceConfig(cfg) -> cfg.url
    TextSourceConfig(_) -> "text://inline"
  }
}

// =============================================================================
// Row Decoders
// =============================================================================

fn source_row_decoder() -> decode.Decoder(Source) {
  use id <- decode.field(0, decode.string)
  use name <- decode.field(1, decode.string)
  use type_str <- decode.field(2, decode.string)
  use config_json <- decode.field(3, decode.string)
  use credentials <- decode.field(4, decode.optional(decode.string))
  use description <- decode.field(5, decode.optional(decode.string))
  use url <- decode.field(6, decode.optional(decode.string))
  use is_active <- decode.field(7, decode.bool)
  use last_verified_at <- decode.field(8, decode.optional(decode.string))
  use last_error <- decode.field(9, decode.optional(decode.string))
  use created_at <- decode.field(10, decode.string)
  use updated_at <- decode.field(11, decode.string)

  let source_type = case source.source_type_from_string(type_str) {
    Ok(t) -> t
    Error(_) -> GitHub
  }

  let config = parse_config(source_type, config_json)

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

fn parse_config(source_type: SourceType, config_json: String) -> SourceConfig {
  case source_type {
    GitHub -> {
      let decoder = {
        use owner <- decode.field("owner", decode.string)
        use repo <- decode.field("repo", decode.string)
        use branch <- decode.optional_field("branch", "main", decode.string)
        use base_path <- decode.optional_field("base_path", "", decode.string)
        decode.success(GitHubConfig(owner, repo, branch, base_path))
      }
      case json.parse(config_json, decoder) {
        Ok(cfg) -> GitHubSourceConfig(cfg)
        Error(_) -> GitHubSourceConfig(GitHubConfig("", "", "main", ""))
      }
    }
    GitLab -> {
      let decoder = {
        use project_id <- decode.field("project_id", decode.string)
        use host <- decode.optional_field(
          "host",
          "https://gitlab.com",
          decode.string,
        )
        use branch <- decode.optional_field("branch", "main", decode.string)
        use base_path <- decode.optional_field("base_path", "", decode.string)
        decode.success(GitLabConfig(project_id, host, branch, base_path))
      }
      case json.parse(config_json, decoder) {
        Ok(cfg) -> GitLabSourceConfig(cfg)
        Error(_) ->
          GitLabSourceConfig(GitLabConfig("", "https://gitlab.com", "main", ""))
      }
    }
    Filesystem -> {
      let decoder = {
        use base_path <- decode.field("base_path", decode.string)
        use allow_writes <- decode.optional_field(
          "allow_writes",
          True,
          decode.bool,
        )
        decode.success(FilesystemConfig(base_path, allow_writes))
      }
      case json.parse(config_json, decoder) {
        Ok(cfg) -> FilesystemSourceConfig(cfg)
        Error(_) -> FilesystemSourceConfig(FilesystemConfig("", True))
      }
    }
    ICal -> {
      let decoder = {
        use url <- decode.field("url", decode.string)
        use refresh_interval <- decode.optional_field(
          "refresh_interval",
          None,
          decode.optional(decode.int),
        )
        decode.success(ICalConfig(url, refresh_interval))
      }
      case json.parse(config_json, decoder) {
        Ok(cfg) -> ICalSourceConfig(cfg)
        Error(_) -> ICalSourceConfig(ICalConfig("", None))
      }
    }
    IMAP -> {
      let decoder = {
        use host <- decode.field("host", decode.string)
        use port <- decode.optional_field("port", 993, decode.int)
        use username <- decode.field("username", decode.string)
        use use_ssl <- decode.optional_field("use_ssl", True, decode.bool)
        use folder <- decode.optional_field(
          "folder",
          None,
          decode.optional(decode.string),
        )
        decode.success(IMAPConfig(host, port, username, use_ssl, folder))
      }
      case json.parse(config_json, decoder) {
        Ok(cfg) -> IMAPSourceConfig(cfg)
        Error(_) -> IMAPSourceConfig(IMAPConfig("", 993, "", True, None))
      }
    }
    Discord -> {
      let decoder = {
        use server_id <- decode.field("server_id", decode.string)
        use channel_ids <- decode.optional_field(
          "channel_ids",
          None,
          decode.optional(decode.list(decode.string)),
        )
        decode.success(DiscordConfig(server_id, channel_ids))
      }
      case json.parse(config_json, decoder) {
        Ok(cfg) -> DiscordSourceConfig(cfg)
        Error(_) -> DiscordSourceConfig(DiscordConfig("", None))
      }
    }
    Slack -> {
      let decoder = {
        use workspace_id <- decode.field("workspace_id", decode.string)
        use channel_ids <- decode.optional_field(
          "channel_ids",
          None,
          decode.optional(decode.list(decode.string)),
        )
        decode.success(SlackConfig(workspace_id, channel_ids))
      }
      case json.parse(config_json, decoder) {
        Ok(cfg) -> SlackSourceConfig(cfg)
        Error(_) -> SlackSourceConfig(SlackConfig("", None))
      }
    }
    Web -> {
      // For web source, just decode url and ignore complex headers for now
      let decoder = {
        use url <- decode.field("url", decode.string)
        decode.success(WebConfig(url, None))
      }
      case json.parse(config_json, decoder) {
        Ok(cfg) -> WebSourceConfig(cfg)
        Error(_) -> WebSourceConfig(WebConfig("", None))
      }
    }
    Text -> {
      let decoder = {
        use content <- decode.field("content", decode.string)
        use label <- decode.optional_field(
          "label",
          None,
          decode.optional(decode.string),
        )
        decode.success(TextConfig(content, label))
      }
      case json.parse(config_json, decoder) {
        Ok(cfg) -> TextSourceConfig(cfg)
        Error(_) -> TextSourceConfig(TextConfig("", None))
      }
    }
  }
}

import gleam/string
