/// Source model for content operations
/// Supports files, calendars, mail, chat, web, and text sources
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}

/// Source categories - high-level grouping of source types
pub type SourceCategory {
  FileCategory
  CalendarCategory
  MailCategory
  ChatCategory
  WebCategory
  TextCategory
}

/// Convert source category to string
pub fn source_category_to_string(category: SourceCategory) -> String {
  case category {
    FileCategory -> "file"
    CalendarCategory -> "calendar"
    MailCategory -> "mail"
    ChatCategory -> "chat"
    WebCategory -> "web"
    TextCategory -> "text"
  }
}

/// Parse source category from string
pub fn source_category_from_string(s: String) -> Result(SourceCategory, String) {
  case s {
    "file" -> Ok(FileCategory)
    "calendar" -> Ok(CalendarCategory)
    "mail" -> Ok(MailCategory)
    "chat" -> Ok(ChatCategory)
    "web" -> Ok(WebCategory)
    "text" -> Ok(TextCategory)
    _ -> Error("Unknown source category: " <> s)
  }
}

/// Source types
pub type SourceType {
  // File sources
  GitHub
  GitLab
  Filesystem
  // Calendar sources
  ICal
  // Mail sources
  IMAP
  // Chat sources (future)
  Discord
  Slack
  // Simple sources
  Web
  Text
}

/// Convert source type to string
pub fn source_type_to_string(source_type: SourceType) -> String {
  case source_type {
    GitHub -> "github"
    GitLab -> "gitlab"
    Filesystem -> "filesystem"
    ICal -> "ical"
    IMAP -> "imap"
    Discord -> "discord"
    Slack -> "slack"
    Web -> "web"
    Text -> "text"
  }
}

/// Parse source type from string
pub fn source_type_from_string(s: String) -> Result(SourceType, String) {
  case s {
    "github" -> Ok(GitHub)
    "gitlab" -> Ok(GitLab)
    "filesystem" -> Ok(Filesystem)
    "ical" -> Ok(ICal)
    "imap" -> Ok(IMAP)
    "discord" -> Ok(Discord)
    "slack" -> Ok(Slack)
    "web" -> Ok(Web)
    "text" -> Ok(Text)
    _ -> Error("Unknown source type: " <> s)
  }
}

/// Get category for a source type
pub fn source_type_category(source_type: SourceType) -> SourceCategory {
  case source_type {
    GitHub | GitLab | Filesystem -> FileCategory
    ICal -> CalendarCategory
    IMAP -> MailCategory
    Discord | Slack -> ChatCategory
    Web -> WebCategory
    Text -> TextCategory
  }
}

/// GitHub-specific configuration
pub type GitHubConfig {
  GitHubConfig(owner: String, repo: String, branch: String, base_path: String)
}

/// GitLab-specific configuration
pub type GitLabConfig {
  GitLabConfig(
    project_id: String,
    host: String,
    branch: String,
    base_path: String,
  )
}

/// Filesystem-specific configuration
pub type FilesystemConfig {
  FilesystemConfig(base_path: String, allow_writes: Bool)
}

/// iCal calendar configuration
pub type ICalConfig {
  ICalConfig(url: String, refresh_interval: Option(Int))
}

/// IMAP mail configuration
pub type IMAPConfig {
  IMAPConfig(
    host: String,
    port: Int,
    username: String,
    use_ssl: Bool,
    folder: Option(String),
  )
}

/// Discord configuration (future)
pub type DiscordConfig {
  DiscordConfig(server_id: String, channel_ids: Option(List(String)))
}

/// Slack configuration (future)
pub type SlackConfig {
  SlackConfig(workspace_id: String, channel_ids: Option(List(String)))
}

/// Web/URL configuration
pub type WebConfig {
  WebConfig(url: String, headers: Option(List(#(String, String))))
}

/// Raw text configuration
pub type TextConfig {
  TextConfig(content: String, label: Option(String))
}

/// Unified source configuration
pub type SourceConfig {
  // File sources
  GitHubSourceConfig(GitHubConfig)
  GitLabSourceConfig(GitLabConfig)
  FilesystemSourceConfig(FilesystemConfig)
  // Calendar sources
  ICalSourceConfig(ICalConfig)
  // Mail sources
  IMAPSourceConfig(IMAPConfig)
  // Chat sources
  DiscordSourceConfig(DiscordConfig)
  SlackSourceConfig(SlackConfig)
  // Simple sources
  WebSourceConfig(WebConfig)
  TextSourceConfig(TextConfig)
}

/// Source model
pub type Source {
  Source(
    id: String,
    name: String,
    source_type: SourceType,
    config: SourceConfig,
    credentials: Option(String),
    description: Option(String),
    url: Option(String),
    is_active: Bool,
    last_verified_at: Option(String),
    last_error: Option(String),
    created_at: String,
    updated_at: String,
  )
}

/// Request to create a source
pub type CreateSourceRequest {
  CreateSourceRequest(
    name: String,
    source_type: SourceType,
    config: SourceConfig,
    credentials: Option(String),
    description: Option(String),
  )
}

/// Request to update a source
pub type UpdateSourceRequest {
  UpdateSourceRequest(
    name: Option(String),
    config: Option(SourceConfig),
    credentials: Option(String),
    description: Option(String),
    is_active: Option(Bool),
  )
}

/// File entry from a source
pub type FileEntry {
  FileEntry(
    path: String,
    name: String,
    is_directory: Bool,
    size: Option(Int),
    sha: Option(String),
  )
}

/// File content from a source
pub type FileContent {
  FileContent(
    path: String,
    content: String,
    sha: Option(String),
    encoding: String,
  )
}

/// Result of a write operation
pub type WriteResult {
  WriteResult(path: String, sha: Option(String), message: String)
}

/// Build URL for a source
pub fn source_url(source: Source) -> String {
  case source.url {
    Some(url) -> url
    None -> {
      case source.config {
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
  }
}

/// Convert config to JSON for database storage
pub fn config_to_json(config: SourceConfig) -> json.Json {
  case config {
    GitHubSourceConfig(cfg) ->
      json.object([
        #("owner", json.string(cfg.owner)),
        #("repo", json.string(cfg.repo)),
        #("branch", json.string(cfg.branch)),
        #("base_path", json.string(cfg.base_path)),
      ])
    GitLabSourceConfig(cfg) ->
      json.object([
        #("project_id", json.string(cfg.project_id)),
        #("host", json.string(cfg.host)),
        #("branch", json.string(cfg.branch)),
        #("base_path", json.string(cfg.base_path)),
      ])
    FilesystemSourceConfig(cfg) ->
      json.object([
        #("base_path", json.string(cfg.base_path)),
        #("allow_writes", json.bool(cfg.allow_writes)),
      ])
    ICalSourceConfig(cfg) ->
      json.object([
        #("url", json.string(cfg.url)),
        #("refresh_interval", case cfg.refresh_interval {
          Some(i) -> json.int(i)
          None -> json.null()
        }),
      ])
    IMAPSourceConfig(cfg) ->
      json.object([
        #("host", json.string(cfg.host)),
        #("port", json.int(cfg.port)),
        #("username", json.string(cfg.username)),
        #("use_ssl", json.bool(cfg.use_ssl)),
        #("folder", case cfg.folder {
          Some(f) -> json.string(f)
          None -> json.null()
        }),
      ])
    DiscordSourceConfig(cfg) ->
      json.object([
        #("server_id", json.string(cfg.server_id)),
        #("channel_ids", case cfg.channel_ids {
          Some(ids) -> json.array(ids, json.string)
          None -> json.null()
        }),
      ])
    SlackSourceConfig(cfg) ->
      json.object([
        #("workspace_id", json.string(cfg.workspace_id)),
        #("channel_ids", case cfg.channel_ids {
          Some(ids) -> json.array(ids, json.string)
          None -> json.null()
        }),
      ])
    WebSourceConfig(cfg) ->
      json.object([
        #("url", json.string(cfg.url)),
        #("headers", case cfg.headers {
          Some(hdrs) ->
            json.object(
              hdrs
              |> list.map(fn(h) { #(h.0, json.string(h.1)) }),
            )
          None -> json.null()
        }),
      ])
    TextSourceConfig(cfg) ->
      json.object([
        #("content", json.string(cfg.content)),
        #("label", case cfg.label {
          Some(l) -> json.string(l)
          None -> json.null()
        }),
      ])
  }
}

/// Source to JSON for API responses
pub fn source_to_json(source: Source) -> json.Json {
  let category = source_type_category(source.source_type)
  json.object([
    #("id", json.string(source.id)),
    #("name", json.string(source.name)),
    #("source_type", json.string(source_type_to_string(source.source_type))),
    #("category", json.string(source_category_to_string(category))),
    #("config", config_to_json(source.config)),
    #("description", case source.description {
      Some(d) -> json.string(d)
      None -> json.null()
    }),
    #("url", json.string(source_url(source))),
    #("is_active", json.bool(source.is_active)),
    #("last_verified_at", case source.last_verified_at {
      Some(t) -> json.string(t)
      None -> json.null()
    }),
    #("last_error", case source.last_error {
      Some(e) -> json.string(e)
      None -> json.null()
    }),
    #("created_at", json.string(source.created_at)),
    #("updated_at", json.string(source.updated_at)),
  ])
}
