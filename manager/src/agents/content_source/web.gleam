/// Web content source
/// Fetches content from URLs
import agents/content_source/types.{
  type ContentSourceError, type SourceHandler, SourceHandler,
}
import gleam/dynamic
import gleam/http
import gleam/http/request
import gleam/http/response
import gleam/httpc
import gleam/list
import gleam/option.{None, Some}
import gleam/result
import gleam/string
import models/content.{
  type ContentItem, type ListQuery, type ListResult, type SearchQuery,
  type WriteResult, ContentItem, ListResult, WebMetadata,
}
import models/source.{
  type Source, type WebConfig, WebCategory, WebSourceConfig,
}

/// List content from a web source (fetches URL and returns single item)
pub fn list_content(
  source: Source,
  _query: ListQuery,
) -> Result(ListResult, ContentSourceError) {
  case source.config {
    WebSourceConfig(cfg) -> {
      case fetch_url(cfg) {
        Ok(item) -> Ok(ListResult(items: [item], total: 1, has_more: False))
        Error(err) -> Error(err)
      }
    }
    _ -> Error(types.InvalidSource("Expected web source"))
  }
}

/// Get content from a web source (same as list, single item)
pub fn get_content(
  source: Source,
  _item_id: String,
) -> Result(ContentItem, ContentSourceError) {
  case source.config {
    WebSourceConfig(cfg) -> fetch_url(cfg)
    _ -> Error(types.InvalidSource("Expected web source"))
  }
}

/// Search content from a web source
/// Fetches the URL and searches within the content
pub fn search_content(
  source: Source,
  query: SearchQuery,
) -> Result(List(ContentItem), ContentSourceError) {
  case source.config {
    WebSourceConfig(cfg) -> {
      case fetch_url(cfg) {
        Ok(item) -> {
          // Simple substring search in fetched content
          case
            string.contains(
              string.lowercase(item.content),
              string.lowercase(query.query),
            )
          {
            True -> Ok([item])
            False -> Ok([])
          }
        }
        Error(err) -> Error(err)
      }
    }
    _ -> Error(types.InvalidSource("Expected web source"))
  }
}

/// Write content to a web source
/// Not supported - web sources are read-only
pub fn write_content(
  _source: Source,
  _item: ContentItem,
) -> Result(WriteResult, ContentSourceError) {
  Error(types.UnsupportedOperation(
    "Web sources are read-only. Cannot write content to a URL.",
  ))
}

/// Fetch content from a URL
fn fetch_url(cfg: WebConfig) -> Result(ContentItem, ContentSourceError) {
  // Parse the URL and create a request
  case request.to(cfg.url) {
    Ok(req) -> {
      // Add custom headers if provided
      let req_with_headers = case cfg.headers {
        Some(headers) ->
          list.fold(headers, req, fn(r, h) { request.set_header(r, h.0, h.1) })
        None -> req
      }

      // Add User-Agent header
      let final_req =
        req_with_headers
        |> request.set_header("User-Agent", "Zone/1.0")

      // Make the request
      case httpc.send(final_req) {
        Ok(resp) -> {
          let content_type = get_content_type(resp)
          let timestamp = types.current_timestamp()

          // Extract headers as list
          let resp_headers =
            resp.headers
            |> list.map(fn(h) { #(h.0, h.1) })

          Ok(ContentItem(
            id: cfg.url,
            source_id: "",
            category: WebCategory,
            title: extract_title(resp.body, cfg.url),
            content: resp.body,
            content_type: content_type,
            timestamp: Some(timestamp),
            url: Some(cfg.url),
            metadata: WebMetadata(
              status_code: resp.status,
              headers: resp_headers,
              fetched_at: timestamp,
            ),
          ))
        }
        Error(_) ->
          Error(types.NetworkError("Failed to fetch URL: " <> cfg.url))
      }
    }
    Error(_) -> Error(types.InvalidSource("Invalid URL: " <> cfg.url))
  }
}

/// Get content type from response headers
fn get_content_type(resp: response.Response(String)) -> String {
  resp.headers
  |> list.find(fn(h) { string.lowercase(h.0) == "content-type" })
  |> result.map(fn(h) { h.1 })
  |> result.unwrap("text/html")
  |> string.split(";")
  |> list.first()
  |> result.unwrap("text/html")
}

/// Extract title from HTML content or use URL as fallback
fn extract_title(content: String, url: String) -> String {
  // Simple title extraction from HTML
  case string.contains(content, "<title>") {
    True -> {
      content
      |> string.split("<title>")
      |> list.drop(1)
      |> list.first()
      |> result.unwrap("")
      |> string.split("</title>")
      |> list.first()
      |> result.unwrap(url)
      |> string.trim()
    }
    False -> {
      // Use the last part of the URL path as title
      url
      |> string.split("/")
      |> list.last()
      |> result.unwrap(url)
      |> string.split("?")
      |> list.first()
      |> result.unwrap(url)
    }
  }
}

/// Get the handler for web sources
pub fn handler() -> SourceHandler {
  SourceHandler(
    list_content: list_content,
    get_content: get_content,
    search_content: search_content,
    write_content: write_content,
  )
}
