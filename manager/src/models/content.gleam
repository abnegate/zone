/// Content model for unified content representation
/// Provides a common abstraction across all source categories
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import models/source.{type SourceCategory, source_category_to_string}

/// Unified content item - represents any content from any source
pub type ContentItem {
  ContentItem(
    id: String,
    source_id: String,
    category: SourceCategory,
    title: String,
    content: String,
    content_type: String,
    timestamp: Option(String),
    url: Option(String),
    metadata: ContentMetadata,
  )
}

/// Category-specific metadata
pub type ContentMetadata {
  /// File metadata (GitHub, GitLab, Filesystem)
  FileMetadata(path: String, size: Int, sha: Option(String), is_directory: Bool)
  /// Calendar event metadata (iCal, Google Calendar, Outlook)
  CalendarMetadata(
    start_time: String,
    end_time: String,
    location: Option(String),
    attendees: List(String),
    recurrence: Option(String),
    all_day: Bool,
  )
  /// Email metadata (IMAP, Gmail, Outlook)
  MailMetadata(
    from_address: String,
    to_addresses: List(String),
    cc_addresses: List(String),
    subject: String,
    thread_id: Option(String),
    attachments: List(String),
    is_read: Bool,
  )
  /// Chat message metadata (Discord, Slack)
  ChatMetadata(
    channel_id: String,
    channel_name: Option(String),
    author_id: String,
    author_name: String,
    thread_id: Option(String),
    reactions: List(String),
  )
  /// Web page metadata (URL fetcher)
  WebMetadata(
    status_code: Int,
    headers: List(#(String, String)),
    fetched_at: String,
  )
  /// Raw text metadata
  TextMetadata(label: Option(String))
}

/// Query for listing content
pub type ListQuery {
  ListQuery(
    path: Option(String),
    start_date: Option(String),
    end_date: Option(String),
    channel: Option(String),
    folder: Option(String),
    limit: Int,
    offset: Int,
  )
}

/// Query for searching content
pub type SearchQuery {
  SearchQuery(
    query: String,
    path: Option(String),
    start_date: Option(String),
    end_date: Option(String),
    limit: Int,
  )
}

/// Result of listing content
pub type ListResult {
  ListResult(items: List(ContentItem), total: Int, has_more: Bool)
}

/// Result of writing content
pub type WriteResult {
  WriteResult(id: String, url: Option(String), message: String)
}

/// Content source error
pub type ContentError {
  NotFound(String)
  AccessDenied(String)
  NetworkError(String)
  ParseError(String)
  WriteError(String)
  InvalidSource(String)
  UnsupportedOperation(String)
  RateLimited(retry_after: Option(Int))
}

/// Default list query
pub fn default_list_query() -> ListQuery {
  ListQuery(
    path: None,
    start_date: None,
    end_date: None,
    channel: None,
    folder: None,
    limit: 50,
    offset: 0,
  )
}

/// Default search query
pub fn default_search_query(query: String) -> SearchQuery {
  SearchQuery(
    query: query,
    path: None,
    start_date: None,
    end_date: None,
    limit: 50,
  )
}

/// Convert error to string
pub fn error_to_string(error: ContentError) -> String {
  case error {
    NotFound(msg) -> "Not found: " <> msg
    AccessDenied(msg) -> "Access denied: " <> msg
    NetworkError(msg) -> "Network error: " <> msg
    ParseError(msg) -> "Parse error: " <> msg
    WriteError(msg) -> "Write error: " <> msg
    InvalidSource(msg) -> "Invalid source: " <> msg
    UnsupportedOperation(msg) -> "Unsupported operation: " <> msg
    RateLimited(Some(seconds)) ->
      "Rate limited, retry after " <> int_to_string(seconds) <> " seconds"
    RateLimited(None) -> "Rate limited"
  }
}

@external(erlang, "erlang", "integer_to_list")
fn int_to_string(n: Int) -> String

/// Convert metadata to JSON
pub fn metadata_to_json(metadata: ContentMetadata) -> json.Json {
  case metadata {
    FileMetadata(path, size, sha, is_directory) ->
      json.object([
        #("type", json.string("file")),
        #("path", json.string(path)),
        #("size", json.int(size)),
        #("sha", option_to_json(sha, json.string)),
        #("is_directory", json.bool(is_directory)),
      ])
    CalendarMetadata(
      start_time,
      end_time,
      location,
      attendees,
      recurrence,
      all_day,
    ) ->
      json.object([
        #("type", json.string("calendar")),
        #("start_time", json.string(start_time)),
        #("end_time", json.string(end_time)),
        #("location", option_to_json(location, json.string)),
        #("attendees", json.array(attendees, json.string)),
        #("recurrence", option_to_json(recurrence, json.string)),
        #("all_day", json.bool(all_day)),
      ])
    MailMetadata(
      from_address,
      to_addresses,
      cc_addresses,
      subject,
      thread_id,
      attachments,
      is_read,
    ) ->
      json.object([
        #("type", json.string("mail")),
        #("from", json.string(from_address)),
        #("to", json.array(to_addresses, json.string)),
        #("cc", json.array(cc_addresses, json.string)),
        #("subject", json.string(subject)),
        #("thread_id", option_to_json(thread_id, json.string)),
        #("attachments", json.array(attachments, json.string)),
        #("is_read", json.bool(is_read)),
      ])
    ChatMetadata(
      channel_id,
      channel_name,
      author_id,
      author_name,
      thread_id,
      reactions,
    ) ->
      json.object([
        #("type", json.string("chat")),
        #("channel_id", json.string(channel_id)),
        #("channel_name", option_to_json(channel_name, json.string)),
        #("author_id", json.string(author_id)),
        #("author_name", json.string(author_name)),
        #("thread_id", option_to_json(thread_id, json.string)),
        #("reactions", json.array(reactions, json.string)),
      ])
    WebMetadata(status_code, headers, fetched_at) ->
      json.object([
        #("type", json.string("web")),
        #("status_code", json.int(status_code)),
        #(
          "headers",
          json.object(
            headers
            |> list.map(fn(h) { #(h.0, json.string(h.1)) }),
          ),
        ),
        #("fetched_at", json.string(fetched_at)),
      ])
    TextMetadata(label) ->
      json.object([
        #("type", json.string("text")),
        #("label", option_to_json(label, json.string)),
      ])
  }
}

/// Helper to convert Option to JSON
fn option_to_json(opt: Option(a), to_json: fn(a) -> json.Json) -> json.Json {
  case opt {
    Some(value) -> to_json(value)
    None -> json.null()
  }
}

/// Convert content item to JSON
pub fn content_item_to_json(item: ContentItem) -> json.Json {
  json.object([
    #("id", json.string(item.id)),
    #("source_id", json.string(item.source_id)),
    #("category", json.string(source_category_to_string(item.category))),
    #("title", json.string(item.title)),
    #("content", json.string(item.content)),
    #("content_type", json.string(item.content_type)),
    #("timestamp", option_to_json(item.timestamp, json.string)),
    #("url", option_to_json(item.url, json.string)),
    #("metadata", metadata_to_json(item.metadata)),
  ])
}

/// Convert list result to JSON
pub fn list_result_to_json(result: ListResult) -> json.Json {
  json.object([
    #("items", json.array(result.items, content_item_to_json)),
    #("total", json.int(result.total)),
    #("has_more", json.bool(result.has_more)),
  ])
}

/// Convert write result to JSON
pub fn write_result_to_json(result: WriteResult) -> json.Json {
  json.object([
    #("id", json.string(result.id)),
    #("url", option_to_json(result.url, json.string)),
    #("message", json.string(result.message)),
  ])
}
