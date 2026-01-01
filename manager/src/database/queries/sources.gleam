import birl
import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/dynamic/decode
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import gleam/time/timestamp.{type Timestamp}
import models/source.{
  type CreateSourceRequest, type Source, type SourceCategory, type SourceConfig,
  type SourceType, type UpdateSourceRequest, Discord, DiscordConfig,
  DiscordSourceConfig, Filesystem, FilesystemConfig, FilesystemSourceConfig,
  GitHub, GitHubConfig, GitHubSourceConfig, GitLab, GitLabConfig,
  GitLabSourceConfig, ICal, ICalConfig, ICalSourceConfig, IMAP, IMAPConfig,
  IMAPSourceConfig, Slack, SlackConfig, SlackSourceConfig, Source, Text,
  TextConfig, TextSourceConfig, Web, WebConfig, WebSourceConfig,
}
import youid/uuid

// =============================================================================
// Source Queries (using Squirrel-generated SQL)
// =============================================================================

/// List all sources, optionally filtered by type
pub fn list_sources(
  db: Connection,
  type_filter: Option(SourceType),
  active_only: Bool,
) -> Result(List(Source), String) {
  case type_filter, active_only {
    None, False ->
      sql.list_sources_all(db)
      |> result.map(fn(returned) {
        list.map(returned.rows, list_sources_all_row_to_source)
      })
      |> result.map_error(query_error_to_string)

    None, True ->
      sql.list_sources_active(db)
      |> result.map(fn(returned) {
        list.map(returned.rows, list_sources_active_row_to_source)
      })
      |> result.map_error(query_error_to_string)

    Some(t), False ->
      sql.list_sources_by_type(db, source.source_type_to_string(t))
      |> result.map(fn(returned) {
        list.map(returned.rows, list_sources_by_type_row_to_source)
      })
      |> result.map_error(query_error_to_string)

    Some(t), True ->
      sql.list_sources_by_type_active(db, source.source_type_to_string(t))
      |> result.map(fn(returned) {
        list.map(returned.rows, list_sources_by_type_active_row_to_source)
      })
      |> result.map_error(query_error_to_string)
  }
}

/// Get a single source by ID
pub fn get_source(db: Connection, id: String) -> Result(Option(Source), String) {
  case uuid.from_string(id) {
    Ok(uuid_id) ->
      sql.get_source_by_id(db, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(get_source_row_to_source)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Get source for a task (task source or fallback to project source)
pub fn get_task_source(
  db: Connection,
  task_id: String,
) -> Result(Option(Source), String) {
  case uuid.from_string(task_id) {
    Ok(uuid_id) ->
      sql.get_task_source(db, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(get_task_source_row_to_source)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Create a new source
pub fn create_source(
  db: Connection,
  req: CreateSourceRequest,
) -> Result(Source, String) {
  let now = timestamp.system_time()
  let type_str = source.source_type_to_string(req.source_type)
  let config_json = source.config_to_json(req.config)
  let url = build_source_url(req.source_type, req.config)
  let credentials = option.unwrap(req.credentials, "")
  let description = option.unwrap(req.description, "")

  sql.create_source(
    db,
    req.name,
    type_str,
    config_json,
    credentials,
    description,
    url,
    now,
    now,
  )
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(row) -> create_source_row_to_source(row)
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
      case uuid.from_string(id) {
        Ok(uuid_id) -> {
          let now = timestamp.system_time()
          let name = option.unwrap(req.name, existing.name)
          let config = option.unwrap(req.config, existing.config)
          let config_json = source.config_to_json(config)
          let credentials = case req.credentials {
            Some(c) -> c
            None -> option.unwrap(existing.credentials, "")
          }
          let description = case req.description {
            Some(d) -> d
            None -> option.unwrap(existing.description, "")
          }
          let is_active = option.unwrap(req.is_active, existing.is_active)
          let url = build_source_url(existing.source_type, config)

          sql.update_source(
            db,
            name,
            config_json,
            credentials,
            description,
            url,
            is_active,
            now,
            uuid_id,
          )
          |> result.map(fn(returned) {
            list.first(returned.rows)
            |> result.map(update_source_row_to_source)
            |> option.from_result
          })
          |> result.map_error(query_error_to_string)
        }
        Error(_) -> Error("Invalid UUID format")
      }
    }
    Ok(None) -> Ok(None)
    Error(err) -> Error(err)
  }
}

/// Delete a source by ID
pub fn delete_source(db: Connection, id: String) -> Result(Bool, String) {
  case uuid.from_string(id) {
    Ok(uuid_id) ->
      sql.delete_source(db, uuid_id)
      |> result.map(fn(returned) { returned.count > 0 })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Verify a source connection and update status
pub fn verify_source(
  db: Connection,
  id: String,
  _success: Bool,
  error_msg: Option(String),
) -> Result(Nil, String) {
  case uuid.from_string(id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      let error_str = option.unwrap(error_msg, "")

      sql.verify_source(db, now, error_str, now, uuid_id)
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Link a source to a project
pub fn link_source_to_project(
  db: Connection,
  project_id: String,
  source_id: String,
) -> Result(Nil, String) {
  case uuid.from_string(source_id), uuid.from_string(project_id) {
    Ok(source_uuid), Ok(project_uuid) -> {
      let now = timestamp.system_time()
      sql.link_source_to_project(db, source_uuid, now, project_uuid)
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    }
    _, _ -> Error("Invalid UUID format")
  }
}

/// Unlink source from a project
pub fn unlink_source_from_project(
  db: Connection,
  project_id: String,
) -> Result(Nil, String) {
  case uuid.from_string(project_id) {
    Ok(uuid_id) -> {
      let now = timestamp.system_time()
      sql.unlink_source_from_project(db, now, uuid_id)
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Link a source to a task
pub fn link_source_to_task(
  db: Connection,
  task_id: String,
  source_id: String,
) -> Result(Nil, String) {
  case uuid.from_string(source_id), uuid.from_string(task_id) {
    Ok(source_uuid), Ok(task_uuid) -> {
      let now = timestamp.system_time()
      sql.link_source_to_task(db, source_uuid, now, task_uuid)
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    }
    _, _ -> Error("Invalid UUID format")
  }
}

/// Link multiple sources to a task
pub fn link_sources_to_task(
  db: Connection,
  task_id: String,
  source_ids: List(String),
) -> Result(Nil, String) {
  case uuid.from_string(task_id) {
    Ok(task_uuid) -> {
      let now = timestamp.system_time()
      // Convert string UUIDs to Uuid types
      let uuids =
        source_ids
        |> list.filter_map(fn(s) { uuid.from_string(s) })
      sql.link_sources_to_task(db, uuids, now, task_uuid)
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Get all sources for a task (from source_ids array or fallback to project source)
pub fn get_task_sources(
  db: Connection,
  task_id: String,
) -> Result(List(Source), String) {
  case uuid.from_string(task_id) {
    Ok(uuid_id) -> {
      // First try to get sources from source_ids array
      let array_result =
        sql.get_task_sources_array(db, uuid_id)
        |> result.map(fn(returned) {
          list.map(returned.rows, get_task_sources_array_row_to_source)
        })
        |> result.map_error(query_error_to_string)

      case array_result {
        Ok([_, ..] as sources) -> Ok(sources)
        Ok([]) | Error(_) -> {
          // Fallback to single source_id or project source
          case get_task_source(db, task_id) {
            Ok(Some(src)) -> Ok([src])
            Ok(None) -> Ok([])
            Error(err) -> Error(err)
          }
        }
      }
    }
    Error(_) -> Error("Invalid UUID format")
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

  case active_only {
    True ->
      sql.list_sources_by_category(db, category_str)
      |> result.map(fn(returned) {
        list.map(returned.rows, list_sources_by_category_row_to_source)
      })
      |> result.map_error(query_error_to_string)

    False ->
      sql.list_sources_by_category_all(db, category_str)
      |> result.map(fn(returned) {
        list.map(returned.rows, list_sources_by_category_all_row_to_source)
      })
      |> result.map_error(query_error_to_string)
  }
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
// Row Mapping Helpers
// =============================================================================

fn timestamp_to_string(ts: Option(Timestamp)) -> String {
  case ts {
    Some(t) -> {
      let #(seconds, _nanoseconds) = timestamp.to_unix_seconds_and_nanoseconds(t)
      birl.from_unix(seconds) |> birl.to_iso8601
    }
    None -> ""
  }
}

fn timestamp_to_option_string(
  ts: Option(Timestamp),
) -> Option(String) {
  case ts {
    Some(t) -> {
      let #(seconds, _nanoseconds) = timestamp.to_unix_seconds_and_nanoseconds(t)
      Some(birl.from_unix(seconds) |> birl.to_iso8601)
    }
    None -> None
  }
}

fn source_type_from_string(type_str: String) -> SourceType {
  case source.source_type_from_string(type_str) {
    Ok(t) -> t
    Error(_) -> GitHub
  }
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

// Row mappers for each query type

fn list_sources_all_row_to_source(row: sql.ListSourcesAllRow) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn list_sources_active_row_to_source(row: sql.ListSourcesActiveRow) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn list_sources_by_type_row_to_source(row: sql.ListSourcesByTypeRow) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn list_sources_by_type_active_row_to_source(
  row: sql.ListSourcesByTypeActiveRow,
) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn get_source_row_to_source(row: sql.GetSourceByIdRow) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn get_task_source_row_to_source(row: sql.GetTaskSourceRow) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn create_source_row_to_source(row: sql.CreateSourceRow) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn update_source_row_to_source(row: sql.UpdateSourceRow) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn get_task_sources_array_row_to_source(
  row: sql.GetTaskSourcesArrayRow,
) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn list_sources_by_category_row_to_source(
  row: sql.ListSourcesByCategoryRow,
) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn list_sources_by_category_all_row_to_source(
  row: sql.ListSourcesByCategoryAllRow,
) -> Source {
  let source_type = source_type_from_string(row.source_type)
  Source(
    id: uuid.to_string(row.id),
    name: row.name,
    source_type: source_type,
    config: parse_config(source_type, row.config),
    credentials: row.credentials_encrypted,
    description: row.description,
    url: row.url,
    is_active: option.unwrap(row.is_active, True),
    last_verified_at: timestamp_to_option_string(row.last_verified_at),
    last_error: row.last_error,
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}
