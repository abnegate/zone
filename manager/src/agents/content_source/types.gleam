/// Shared types for content source operations
import gleam/option.{type Option}
import models/content.{
  type ContentItem, type ListQuery, type ListResult, type SearchQuery,
  type WriteResult,
}
import models/source.{type Source}

/// Handler functions for a content source type
/// Each source type module exports one of these
pub type SourceHandler {
  SourceHandler(
    list_content: fn(Source, ListQuery) ->
      Result(ListResult, ContentSourceError),
    get_content: fn(Source, String) -> Result(ContentItem, ContentSourceError),
    search_content: fn(Source, SearchQuery) ->
      Result(List(ContentItem), ContentSourceError),
    write_content: fn(Source, ContentItem) ->
      Result(WriteResult, ContentSourceError),
  )
}

/// Content source error types
pub type ContentSourceError {
  NotFound(String)
  AccessDenied(String)
  NetworkError(String)
  ParseError(String)
  WriteError(String)
  InvalidSource(String)
  UnsupportedOperation(String)
  RateLimited(retry_after: Option(Int))
}

/// Convert error to human-readable string
pub fn error_to_string(error: ContentSourceError) -> String {
  case error {
    NotFound(msg) -> "Not found: " <> msg
    AccessDenied(msg) -> "Access denied: " <> msg
    NetworkError(msg) -> "Network error: " <> msg
    ParseError(msg) -> "Parse error: " <> msg
    WriteError(msg) -> "Write error: " <> msg
    InvalidSource(msg) -> "Invalid source: " <> msg
    UnsupportedOperation(msg) -> "Unsupported operation: " <> msg
    RateLimited(option.Some(seconds)) ->
      "Rate limited, retry after " <> int_to_string(seconds) <> " seconds"
    RateLimited(option.None) -> "Rate limited"
  }
}

@external(erlang, "erlang", "integer_to_list")
fn int_to_string(n: Int) -> String

/// URI encode a string (using Erlang's http_uri module)
@external(erlang, "http_uri", "encode")
pub fn uri_encode(s: String) -> String

/// Base64 encode
@external(erlang, "base64", "encode")
pub fn base64_encode(data: BitArray) -> String

/// Base64 decode
@external(erlang, "base64", "decode")
pub fn base64_decode(data: String) -> BitArray

/// Get current timestamp in ISO8601 format
@external(erlang, "calendar", "universal_time")
fn erlang_universal_time() -> #(#(Int, Int, Int), #(Int, Int, Int))

pub fn current_timestamp() -> String {
  let #(#(year, month, day), #(hour, minute, second)) = erlang_universal_time()
  int_to_string(year)
  <> "-"
  <> pad_zero(month)
  <> "-"
  <> pad_zero(day)
  <> "T"
  <> pad_zero(hour)
  <> ":"
  <> pad_zero(minute)
  <> ":"
  <> pad_zero(second)
  <> "Z"
}

fn pad_zero(n: Int) -> String {
  case n < 10 {
    True -> "0" <> int_to_string(n)
    False -> int_to_string(n)
  }
}
