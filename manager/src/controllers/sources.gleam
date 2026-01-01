import agents/content_source
import agents/file_source
import cache/queries/sources as cached_sources
import gleam/dynamic/decode
import gleam/http
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/string
import models/content
import models/source.{
  type CreateSourceRequest, type SourceType, type UpdateSourceRequest,
  CreateSourceRequest, Discord, DiscordConfig, DiscordSourceConfig, Filesystem,
  FilesystemConfig, FilesystemSourceConfig, GitHub, GitHubConfig,
  GitHubSourceConfig, GitLab, GitLabConfig, GitLabSourceConfig, ICal, ICalConfig,
  ICalSourceConfig, IMAP, IMAPConfig, IMAPSourceConfig, Slack, SlackConfig,
  SlackSourceConfig, Text, TextConfig, TextSourceConfig, UpdateSourceRequest,
  Web, WebConfig, WebSourceConfig,
}
import web.{type Context}
import wisp.{type Request, type Response}

/// Handle all /api/sources routes
pub fn handle_sources_route(
  req: Request,
  path: List(String),
  ctx: Context,
) -> Response {
  case path {
    [] -> handle_sources_collection(req, ctx)
    ["types"] -> list_source_types(req)
    [id] -> handle_single_source(req, id, ctx)
    [id, "verify"] -> verify_source(req, id, ctx)
    _ -> wisp.not_found()
  }
}

/// Handle /api/sources (collection)
fn handle_sources_collection(req: Request, ctx: Context) -> Response {
  case req.method {
    http.Get -> list_sources(req, ctx)
    http.Post -> create_source(req, ctx)
    _ -> wisp.method_not_allowed([http.Get, http.Post])
  }
}

/// Handle /api/sources/:id (single source)
fn handle_single_source(req: Request, id: String, ctx: Context) -> Response {
  case req.method {
    http.Get -> get_source(id, ctx)
    http.Patch -> update_source(req, id, ctx)
    http.Delete -> delete_source(id, ctx)
    _ -> wisp.method_not_allowed([http.Get, http.Patch, http.Delete])
  }
}

// =============================================================================
// Handlers
// =============================================================================

/// GET /api/sources/types - List available source types
fn list_source_types(_req: Request) -> Response {
  let types =
    json.array(
      [
        // File sources
        #("github", "GitHub Repository", "file", True),
        #("gitlab", "GitLab Repository", "file", True),
        #("filesystem", "Local Filesystem", "file", True),
        // Calendar sources
        #("ical", "iCalendar URL", "calendar", True),
        // Mail sources
        #("imap", "IMAP Mail Server", "mail", True),
        // Chat sources (future)
        #("discord", "Discord Server", "chat", False),
        #("slack", "Slack Workspace", "chat", False),
        // Simple sources
        #("web", "Web URL", "web", True),
        #("text", "Raw Text", "text", True),
      ],
      fn(t) {
        let #(id, name, category, enabled) = t
        json.object([
          #("id", json.string(id)),
          #("name", json.string(name)),
          #("category", json.string(category)),
          #("enabled", json.bool(enabled)),
        ])
      },
    )

  web.json_success([#("types", types)])
}

/// GET /api/sources - List all sources
fn list_sources(req: Request, ctx: Context) -> Response {
  let query = wisp.get_query(req)

  let type_filter = case find_query_param(query, "type") {
    Some(type_str) -> {
      case source.source_type_from_string(type_str) {
        Ok(t) -> Some(t)
        Error(_) -> None
      }
    }
    None -> None
  }

  let category_filter = case find_query_param(query, "category") {
    Some(cat_str) -> {
      case source.source_category_from_string(cat_str) {
        Ok(c) -> Some(c)
        Error(_) -> None
      }
    }
    None -> None
  }

  let active_only = case find_query_param(query, "active") {
    Some("true") -> True
    _ -> False
  }

  // Use category filter if provided, otherwise use type filter
  case category_filter {
    Some(category) -> {
      case
        cached_sources.list_sources_by_category(
          ctx.db,
          ctx.cache,
          category,
          active_only,
        )
      {
        Ok(source_list) ->
          web.json_success([
            #("sources", json.array(source_list, source.source_to_json)),
          ])
        Error(err) -> web.internal_error(err)
      }
    }
    None -> {
      case
        cached_sources.list_sources(ctx.db, ctx.cache, type_filter, active_only)
      {
        Ok(source_list) ->
          web.json_success([
            #("sources", json.array(source_list, source.source_to_json)),
          ])
        Error(err) -> web.internal_error(err)
      }
    }
  }
}

/// GET /api/sources/:id - Get a single source
fn get_source(id: String, ctx: Context) -> Response {
  case cached_sources.get_source(ctx.db, ctx.cache, id) {
    Ok(Some(src)) -> web.json_success([#("source", source.source_to_json(src))])
    Ok(None) -> web.not_found("Source not found")
    Error(err) -> web.internal_error(err)
  }
}

/// POST /api/sources - Create a new source
fn create_source(req: Request, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case decode_create_request(body) {
    Ok(create_req) -> {
      case cached_sources.create_source(ctx.db, ctx.cache, create_req) {
        Ok(src) -> web.json_created([#("source", source.source_to_json(src))])
        Error(err) -> web.internal_error(err)
      }
    }
    Error(err) -> web.bad_request("Invalid request body: " <> err)
  }
}

/// PATCH /api/sources/:id - Update a source
fn update_source(req: Request, id: String, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case decode_update_request(body) {
    Ok(update_req) -> {
      case cached_sources.update_source(ctx.db, ctx.cache, id, update_req) {
        Ok(Some(src)) ->
          web.json_success([#("source", source.source_to_json(src))])
        Ok(None) -> web.not_found("Source not found")
        Error(err) -> web.internal_error(err)
      }
    }
    Error(err) -> web.bad_request("Invalid request body: " <> err)
  }
}

/// DELETE /api/sources/:id - Delete a source
fn delete_source(id: String, ctx: Context) -> Response {
  case cached_sources.delete_source(ctx.db, ctx.cache, id) {
    Ok(True) -> wisp.no_content()
    Ok(False) -> web.not_found("Source not found")
    Error(err) -> web.internal_error(err)
  }
}

/// POST /api/sources/:id/verify - Verify source connection
fn verify_source(_req: Request, id: String, ctx: Context) -> Response {
  case cached_sources.get_source(ctx.db, ctx.cache, id) {
    Ok(Some(src)) -> {
      // Use content_source abstraction to verify any source type
      let query = content.default_list_query()
      case content_source.list_content(src, query) {
        Ok(result) -> {
          let _ =
            cached_sources.verify_source(ctx.db, ctx.cache, id, True, None)
          web.json_success([
            #("success", json.bool(True)),
            #("message", json.string("Connection verified")),
            #("item_count", json.int(result.total)),
          ])
        }
        Error(err) -> {
          let error_msg = content_source.error_to_string(err)
          let _ =
            cached_sources.verify_source(
              ctx.db,
              ctx.cache,
              id,
              False,
              Some(error_msg),
            )
          web.json_success([
            #("success", json.bool(False)),
            #("message", json.string(error_msg)),
          ])
        }
      }
    }
    Ok(None) -> web.not_found("Source not found")
    Error(err) -> web.internal_error(err)
  }
}

// =============================================================================
// Decoders
// =============================================================================

fn decode_create_request(body: String) -> Result(CreateSourceRequest, String) {
  // First decode the basic fields
  let basic_decoder = {
    use name <- decode.field("name", decode.string)
    use type_str <- decode.field("source_type", decode.string)
    use config_raw <- decode.field("config", decode.dynamic)
    use credentials <- decode.optional_field(
      "credentials",
      None,
      decode.optional(decode.string),
    )
    use description <- decode.optional_field(
      "description",
      None,
      decode.optional(decode.string),
    )
    decode.success(#(name, type_str, config_raw, credentials, description))
  }

  case json.parse(body, basic_decoder) {
    Ok(#(name, type_str, config_raw, credentials, description)) -> {
      case source.source_type_from_string(type_str) {
        Ok(source_type) -> {
          case decode_config(source_type, config_raw) {
            Ok(config) ->
              Ok(CreateSourceRequest(
                name,
                source_type,
                config,
                credentials,
                description,
              ))
            Error(e) -> Error("Config error: " <> e)
          }
        }
        Error(e) -> Error("Invalid source_type: " <> e)
      }
    }
    Error(e) -> Error("Parse error: " <> string.inspect(e))
  }
}

fn decode_update_request(body: String) -> Result(UpdateSourceRequest, String) {
  let decoder = {
    use name <- decode.optional_field(
      "name",
      None,
      decode.optional(decode.string),
    )
    use credentials <- decode.optional_field(
      "credentials",
      None,
      decode.optional(decode.string),
    )
    use description <- decode.optional_field(
      "description",
      None,
      decode.optional(decode.string),
    )
    use is_active <- decode.optional_field(
      "is_active",
      None,
      decode.optional(decode.bool),
    )

    decode.success(UpdateSourceRequest(
      name,
      None,
      credentials,
      description,
      is_active,
    ))
  }

  case json.parse(body, decoder) {
    Ok(req) -> Ok(req)
    Error(e) -> Error("Parse error: " <> string.inspect(e))
  }
}

fn decode_config(
  source_type: SourceType,
  config_raw: decode.Dynamic,
) -> Result(source.SourceConfig, String) {
  case source_type {
    GitHub -> {
      let decoder = {
        use owner <- decode.field("owner", decode.string)
        use repo <- decode.field("repo", decode.string)
        use branch <- decode.optional_field("branch", "main", decode.string)
        use base_path <- decode.optional_field("base_path", "", decode.string)
        decode.success(
          GitHubSourceConfig(GitHubConfig(owner, repo, branch, base_path)),
        )
      }
      case decode.run(config_raw, decoder) {
        Ok(cfg) -> Ok(cfg)
        Error(_) -> Error("Invalid GitHub config")
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
        decode.success(
          GitLabSourceConfig(GitLabConfig(project_id, host, branch, base_path)),
        )
      }
      case decode.run(config_raw, decoder) {
        Ok(cfg) -> Ok(cfg)
        Error(_) -> Error("Invalid GitLab config")
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
        decode.success(
          FilesystemSourceConfig(FilesystemConfig(base_path, allow_writes)),
        )
      }
      case decode.run(config_raw, decoder) {
        Ok(cfg) -> Ok(cfg)
        Error(_) -> Error("Invalid filesystem config")
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
        decode.success(ICalSourceConfig(ICalConfig(url, refresh_interval)))
      }
      case decode.run(config_raw, decoder) {
        Ok(cfg) -> Ok(cfg)
        Error(_) -> Error("Invalid iCal config")
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
        decode.success(
          IMAPSourceConfig(IMAPConfig(host, port, username, use_ssl, folder)),
        )
      }
      case decode.run(config_raw, decoder) {
        Ok(cfg) -> Ok(cfg)
        Error(_) -> Error("Invalid IMAP config")
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
        decode.success(
          DiscordSourceConfig(DiscordConfig(server_id, channel_ids)),
        )
      }
      case decode.run(config_raw, decoder) {
        Ok(cfg) -> Ok(cfg)
        Error(_) -> Error("Invalid Discord config")
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
        decode.success(
          SlackSourceConfig(SlackConfig(workspace_id, channel_ids)),
        )
      }
      case decode.run(config_raw, decoder) {
        Ok(cfg) -> Ok(cfg)
        Error(_) -> Error("Invalid Slack config")
      }
    }
    Web -> {
      let decoder = {
        use url <- decode.field("url", decode.string)
        decode.success(WebSourceConfig(WebConfig(url, None)))
      }
      case decode.run(config_raw, decoder) {
        Ok(cfg) -> Ok(cfg)
        Error(_) -> Error("Invalid web config")
      }
    }
    Text -> {
      let decoder = {
        use text_content <- decode.field("content", decode.string)
        use label <- decode.optional_field(
          "label",
          None,
          decode.optional(decode.string),
        )
        decode.success(TextSourceConfig(TextConfig(text_content, label)))
      }
      case decode.run(config_raw, decoder) {
        Ok(cfg) -> Ok(cfg)
        Error(_) -> Error("Invalid text config")
      }
    }
  }
}

// =============================================================================
// Helpers
// =============================================================================

fn find_query_param(
  query: List(#(String, String)),
  key: String,
) -> Option(String) {
  case query {
    [] -> None
    [#(k, v), ..rest] -> {
      case k == key {
        True -> Some(v)
        False -> find_query_param(rest, key)
      }
    }
  }
}
