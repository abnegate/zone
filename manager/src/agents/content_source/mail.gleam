/// Mail content source
/// Provides access to emails from IMAP and other mail sources
import agents/content_source/types.{
  type ContentSourceError, type SourceHandler, SourceHandler,
}
import gleam/dynamic
import gleam/erlang/process
import gleam/int
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import models/content.{
  type ContentItem, type ListQuery, type ListResult, type SearchQuery,
  type WriteResult, ContentItem, ListResult, MailMetadata,
}
import models/source.{
  type IMAPConfig, type Source, IMAPSourceConfig, MailCategory,
}

/// External IMAP connection type (opaque)
pub type IMAPConnection

/// List emails from a mail source
pub fn list_content(
  source: Source,
  query: ListQuery,
) -> Result(ListResult, ContentSourceError) {
  case source.config {
    IMAPSourceConfig(cfg) -> {
      let folder =
        option.unwrap(query.folder, option.unwrap(cfg.folder, "INBOX"))
      list_imap_messages(source.id, source.credentials, cfg, folder, query)
    }
    _ -> Error(types.InvalidSource("Expected mail source"))
  }
}

/// Get a specific email by message ID
pub fn get_content(
  source: Source,
  item_id: String,
) -> Result(ContentItem, ContentSourceError) {
  case source.config {
    IMAPSourceConfig(cfg) -> {
      let folder = option.unwrap(cfg.folder, "INBOX")
      get_imap_message(source.id, source.credentials, cfg, folder, item_id)
    }
    _ -> Error(types.InvalidSource("Expected mail source"))
  }
}

/// Search emails by text
pub fn search_content(
  source: Source,
  query: SearchQuery,
) -> Result(List(ContentItem), ContentSourceError) {
  case source.config {
    IMAPSourceConfig(cfg) -> {
      let folder = option.unwrap(cfg.folder, "INBOX")
      search_imap_messages(source.id, source.credentials, cfg, folder, query)
    }
    _ -> Error(types.InvalidSource("Expected mail source"))
  }
}

/// Send/compose email
/// Note: Requires SMTP configuration which is separate
pub fn write_content(
  _source: Source,
  _item: ContentItem,
) -> Result(WriteResult, ContentSourceError) {
  Error(types.UnsupportedOperation(
    "Sending mail requires SMTP configuration. IMAP is read-only.",
  ))
}

/// List messages from IMAP
fn list_imap_messages(
  source_id: String,
  credentials: Option(String),
  cfg: IMAPConfig,
  folder: String,
  query: ListQuery,
) -> Result(ListResult, ContentSourceError) {
  // Connect to IMAP server
  case connect_imap(cfg, credentials) {
    Ok(conn) -> {
      // Select folder
      case imap_select(conn, folder) {
        Ok(message_count) -> {
          // Calculate range (IMAP uses 1-based indexing, newest first)
          let start = int.max(1, message_count - query.offset - query.limit + 1)
          let end = int.max(1, message_count - query.offset)

          // Fetch message headers
          case imap_fetch_headers(conn, start, end) {
            Ok(headers) -> {
              let items =
                headers
                |> list.reverse()
                |> list.map(fn(h) {
                  header_to_content_item(source_id, folder, h)
                })

              imap_logout(conn)
              Ok(ListResult(
                items: items,
                total: message_count,
                has_more: query.offset + query.limit < message_count,
              ))
            }
            Error(err) -> {
              imap_logout(conn)
              Error(err)
            }
          }
        }
        Error(err) -> {
          imap_logout(conn)
          Error(err)
        }
      }
    }
    Error(err) -> Error(err)
  }
}

/// Get a specific message from IMAP
fn get_imap_message(
  source_id: String,
  credentials: Option(String),
  cfg: IMAPConfig,
  folder: String,
  message_id: String,
) -> Result(ContentItem, ContentSourceError) {
  case connect_imap(cfg, credentials) {
    Ok(conn) -> {
      case imap_select(conn, folder) {
        Ok(_) -> {
          // Search for message by Message-ID header
          case imap_search_header(conn, "Message-ID", message_id) {
            Ok([seq_num, ..]) -> {
              case imap_fetch_full(conn, seq_num) {
                Ok(message) -> {
                  imap_logout(conn)
                  Ok(message_to_content_item(source_id, folder, message))
                }
                Error(err) -> {
                  imap_logout(conn)
                  Error(err)
                }
              }
            }
            Ok([]) -> {
              imap_logout(conn)
              Error(types.NotFound("Message not found: " <> message_id))
            }
            Error(err) -> {
              imap_logout(conn)
              Error(err)
            }
          }
        }
        Error(err) -> {
          imap_logout(conn)
          Error(err)
        }
      }
    }
    Error(err) -> Error(err)
  }
}

/// Search messages in IMAP
fn search_imap_messages(
  source_id: String,
  credentials: Option(String),
  cfg: IMAPConfig,
  folder: String,
  query: SearchQuery,
) -> Result(List(ContentItem), ContentSourceError) {
  case connect_imap(cfg, credentials) {
    Ok(conn) -> {
      case imap_select(conn, folder) {
        Ok(_) -> {
          // IMAP SEARCH command
          case imap_search_text(conn, query.query, query.limit) {
            Ok(seq_nums) -> {
              // Fetch headers for matching messages
              let items =
                seq_nums
                |> list.filter_map(fn(seq_num) {
                  case imap_fetch_headers(conn, seq_num, seq_num) {
                    Ok([header]) ->
                      Ok(header_to_content_item(source_id, folder, header))
                    _ -> Error(Nil)
                  }
                })

              imap_logout(conn)
              Ok(items)
            }
            Error(err) -> {
              imap_logout(conn)
              Error(err)
            }
          }
        }
        Error(err) -> {
          imap_logout(conn)
          Error(err)
        }
      }
    }
    Error(err) -> Error(err)
  }
}

// IMAP protocol helpers using Erlang's gen_tcp and ssl modules
// These are simplified implementations - production use should use a proper IMAP library

/// Message header data
pub type MessageHeader {
  MessageHeader(
    seq_num: Int,
    message_id: String,
    from: String,
    to: List(String),
    cc: List(String),
    subject: String,
    date: String,
    flags: List(String),
  )
}

/// Full message data
pub type Message {
  Message(header: MessageHeader, body: String, attachments: List(String))
}

/// Connect to IMAP server
fn connect_imap(
  cfg: IMAPConfig,
  credentials: Option(String),
) -> Result(IMAPConnection, ContentSourceError) {
  let password = option.unwrap(credentials, "")

  case cfg.use_ssl {
    True -> imap_connect_ssl(cfg.host, cfg.port, cfg.username, password)
    False -> imap_connect_plain(cfg.host, cfg.port, cfg.username, password)
  }
}

// FFI functions for IMAP operations
// These would be implemented in Erlang/Elixir using :gen_tcp/:ssl

@external(erlang, "voiz_imap", "connect_ssl")
fn imap_connect_ssl(
  host: String,
  port: Int,
  username: String,
  password: String,
) -> Result(IMAPConnection, ContentSourceError)

@external(erlang, "voiz_imap", "connect_plain")
fn imap_connect_plain(
  host: String,
  port: Int,
  username: String,
  password: String,
) -> Result(IMAPConnection, ContentSourceError)

@external(erlang, "voiz_imap", "select")
fn imap_select(
  conn: IMAPConnection,
  folder: String,
) -> Result(Int, ContentSourceError)

@external(erlang, "voiz_imap", "fetch_headers")
fn imap_fetch_headers(
  conn: IMAPConnection,
  start: Int,
  end: Int,
) -> Result(List(MessageHeader), ContentSourceError)

@external(erlang, "voiz_imap", "fetch_full")
fn imap_fetch_full(
  conn: IMAPConnection,
  seq_num: Int,
) -> Result(Message, ContentSourceError)

@external(erlang, "voiz_imap", "search_header")
fn imap_search_header(
  conn: IMAPConnection,
  header: String,
  value: String,
) -> Result(List(Int), ContentSourceError)

@external(erlang, "voiz_imap", "search_text")
fn imap_search_text(
  conn: IMAPConnection,
  query: String,
  limit: Int,
) -> Result(List(Int), ContentSourceError)

@external(erlang, "voiz_imap", "logout")
fn imap_logout(conn: IMAPConnection) -> Nil

/// Convert header to ContentItem
fn header_to_content_item(
  source_id: String,
  folder: String,
  header: MessageHeader,
) -> ContentItem {
  let is_read = list.contains(header.flags, "\\Seen")

  ContentItem(
    id: header.message_id,
    source_id: source_id,
    category: MailCategory,
    title: header.subject,
    content: "",
    content_type: "message/rfc822",
    timestamp: Some(header.date),
    url: None,
    metadata: MailMetadata(
      from_address: header.from,
      to_addresses: header.to,
      cc_addresses: header.cc,
      subject: header.subject,
      thread_id: None,
      attachments: [],
      is_read: is_read,
    ),
  )
}

/// Convert full message to ContentItem
fn message_to_content_item(
  source_id: String,
  folder: String,
  message: Message,
) -> ContentItem {
  let is_read = list.contains(message.header.flags, "\\Seen")

  ContentItem(
    id: message.header.message_id,
    source_id: source_id,
    category: MailCategory,
    title: message.header.subject,
    content: message.body,
    content_type: "message/rfc822",
    timestamp: Some(message.header.date),
    url: None,
    metadata: MailMetadata(
      from_address: message.header.from,
      to_addresses: message.header.to,
      cc_addresses: message.header.cc,
      subject: message.header.subject,
      thread_id: None,
      attachments: message.attachments,
      is_read: is_read,
    ),
  )
}

/// Get the handler for mail sources
pub fn handler() -> SourceHandler {
  SourceHandler(
    list_content: list_content,
    get_content: get_content,
    search_content: search_content,
    write_content: write_content,
  )
}
