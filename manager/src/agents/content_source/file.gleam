/// File content source adapter
/// Wraps the existing file_source module to provide ContentItem interface
import agents/content_source/types.{
  type ContentSourceError, type SourceHandler, SourceHandler,
}
import agents/file_source
import agents/file_source/types as fs_types
import gleam/list
import gleam/option.{None, Some}
import gleam/string
import models/content.{
  type ContentItem, type ListQuery, type ListResult, type SearchQuery,
  type WriteResult, ContentItem, FileMetadata, ListResult, WriteResult,
}
import models/source.{type Source, FileCategory}

/// Convert file source error to content source error
fn convert_error(error: file_source.FileSourceError) -> ContentSourceError {
  case error {
    fs_types.NotFound(msg) -> types.NotFound(msg)
    fs_types.AccessDenied(msg) -> types.AccessDenied(msg)
    fs_types.NetworkError(msg) -> types.NetworkError(msg)
    fs_types.ParseError(msg) -> types.ParseError(msg)
    fs_types.WriteError(msg) -> types.WriteError(msg)
    fs_types.InvalidSource(msg) -> types.InvalidSource(msg)
    fs_types.UnsupportedOperation(msg) -> types.UnsupportedOperation(msg)
  }
}

/// List content (files) from a file source
pub fn list_content(
  source: Source,
  query: ListQuery,
) -> Result(ListResult, ContentSourceError) {
  let path = option.unwrap(query.path, "")

  case file_source.list_files(source, path) {
    Ok(entries) -> {
      let items =
        entries
        |> list.drop(query.offset)
        |> list.take(query.limit)
        |> list.map(fn(entry) {
          ContentItem(
            id: entry.path,
            source_id: source.id,
            category: FileCategory,
            title: entry.name,
            content: "",
            content_type: case entry.is_directory {
              True -> "inode/directory"
              False -> guess_mime_type(entry.name)
            },
            timestamp: None,
            url: None,
            metadata: FileMetadata(
              path: entry.path,
              size: option.unwrap(entry.size, 0),
              sha: entry.sha,
              is_directory: entry.is_directory,
            ),
          )
        })

      let total = list.length(entries)
      let has_more = query.offset + query.limit < total

      Ok(ListResult(items: items, total: total, has_more: has_more))
    }
    Error(err) -> Error(convert_error(err))
  }
}

/// Get a specific file by path
pub fn get_content(
  source: Source,
  item_id: String,
) -> Result(ContentItem, ContentSourceError) {
  case file_source.read_file(source, item_id) {
    Ok(file_content) -> {
      let name =
        item_id
        |> string.split("/")
        |> list.last()
        |> option.from_result()
        |> option.unwrap("unknown")

      Ok(ContentItem(
        id: item_id,
        source_id: source.id,
        category: FileCategory,
        title: name,
        content: file_content.content,
        content_type: guess_mime_type(name),
        timestamp: None,
        url: None,
        metadata: FileMetadata(
          path: file_content.path,
          size: string.byte_size(file_content.content),
          sha: file_content.sha,
          is_directory: False,
        ),
      ))
    }
    Error(err) -> Error(convert_error(err))
  }
}

/// Search files by content
pub fn search_content(
  source: Source,
  query: SearchQuery,
) -> Result(List(ContentItem), ContentSourceError) {
  case file_source.search_files(source, query.query, query.path) {
    Ok(results) -> {
      let items =
        results
        |> list.take(query.limit)
        |> list.map(fn(result) {
          let #(path, snippet) = result
          let name =
            path
            |> string.split("/")
            |> list.last()
            |> option.from_result()
            |> option.unwrap("unknown")

          ContentItem(
            id: path,
            source_id: source.id,
            category: FileCategory,
            title: name,
            content: snippet,
            content_type: guess_mime_type(name),
            timestamp: None,
            url: None,
            metadata: FileMetadata(
              path: path,
              size: 0,
              sha: None,
              is_directory: False,
            ),
          )
        })

      Ok(items)
    }
    Error(err) -> Error(convert_error(err))
  }
}

/// Write file content
pub fn write_content(
  source: Source,
  item: ContentItem,
) -> Result(WriteResult, ContentSourceError) {
  // Extract path from metadata
  let path = case item.metadata {
    FileMetadata(path, _, _, _) -> path
    _ -> item.id
  }

  case
    file_source.write_file(source, path, item.content, "Update " <> item.title)
  {
    Ok(result) -> {
      Ok(WriteResult(id: result.path, url: None, message: result.message))
    }
    Error(err) -> Error(convert_error(err))
  }
}

/// Guess MIME type from filename
fn guess_mime_type(filename: String) -> String {
  let ext =
    filename
    |> string.split(".")
    |> list.last()
    |> option.from_result()
    |> option.unwrap("")
    |> string.lowercase()

  case ext {
    "txt" -> "text/plain"
    "md" | "markdown" -> "text/markdown"
    "html" | "htm" -> "text/html"
    "css" -> "text/css"
    "js" | "mjs" -> "application/javascript"
    "ts" | "tsx" -> "application/typescript"
    "json" -> "application/json"
    "xml" -> "application/xml"
    "yaml" | "yml" -> "application/yaml"
    "py" -> "text/x-python"
    "rb" -> "text/x-ruby"
    "go" -> "text/x-go"
    "rs" -> "text/x-rust"
    "gleam" -> "text/x-gleam"
    "c" -> "text/x-c"
    "cpp" | "cc" | "cxx" -> "text/x-c++"
    "h" | "hpp" -> "text/x-c-header"
    "java" -> "text/x-java"
    "sh" | "bash" -> "text/x-shellscript"
    "sql" -> "application/sql"
    "png" -> "image/png"
    "jpg" | "jpeg" -> "image/jpeg"
    "gif" -> "image/gif"
    "svg" -> "image/svg+xml"
    "pdf" -> "application/pdf"
    "zip" -> "application/zip"
    "gz" -> "application/gzip"
    _ -> "application/octet-stream"
  }
}

/// Get the handler for file sources
pub fn handler() -> SourceHandler {
  SourceHandler(
    list_content: list_content,
    get_content: get_content,
    search_content: search_content,
    write_content: write_content,
  )
}
