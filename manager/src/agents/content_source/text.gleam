/// Text content source
/// Provides access to raw text/string content stored directly in the source config
import agents/content_source/types.{
  type ContentSourceError, type SourceHandler, SourceHandler,
}
import gleam/option.{None, Some}
import gleam/string
import models/content.{
  type ContentItem, type ListQuery, type ListResult, type SearchQuery,
  type WriteResult, ContentItem, ListResult, TextMetadata,
}
import models/source.{
  type Source, type TextConfig, TextCategory, TextSourceConfig,
}

/// List content from a text source (returns single item)
pub fn list_content(
  source: Source,
  _query: ListQuery,
) -> Result(ListResult, ContentSourceError) {
  case source.config {
    TextSourceConfig(cfg) -> {
      let item = text_config_to_item(source.id, cfg)
      Ok(ListResult(items: [item], total: 1, has_more: False))
    }
    _ -> Error(types.InvalidSource("Expected text source"))
  }
}

/// Get content from a text source
pub fn get_content(
  source: Source,
  _item_id: String,
) -> Result(ContentItem, ContentSourceError) {
  case source.config {
    TextSourceConfig(cfg) -> {
      Ok(text_config_to_item(source.id, cfg))
    }
    _ -> Error(types.InvalidSource("Expected text source"))
  }
}

/// Search content in a text source
pub fn search_content(
  source: Source,
  query: SearchQuery,
) -> Result(List(ContentItem), ContentSourceError) {
  case source.config {
    TextSourceConfig(cfg) -> {
      // Simple substring search
      case string.contains(cfg.content, query.query) {
        True -> Ok([text_config_to_item(source.id, cfg)])
        False -> Ok([])
      }
    }
    _ -> Error(types.InvalidSource("Expected text source"))
  }
}

/// Write content to a text source
/// Note: Text sources are read-only by design (content is in config)
pub fn write_content(
  _source: Source,
  _item: ContentItem,
) -> Result(WriteResult, ContentSourceError) {
  Error(types.UnsupportedOperation(
    "Text sources are read-only. To update content, modify the source configuration.",
  ))
}

/// Convert text config to ContentItem
fn text_config_to_item(source_id: String, cfg: TextConfig) -> ContentItem {
  let title = case cfg.label {
    Some(label) -> label
    None -> "Text content"
  }

  ContentItem(
    id: "content",
    source_id: source_id,
    category: TextCategory,
    title: title,
    content: cfg.content,
    content_type: "text/plain",
    timestamp: None,
    url: None,
    metadata: TextMetadata(label: cfg.label),
  )
}

/// Get the handler for text sources
pub fn handler() -> SourceHandler {
  SourceHandler(
    list_content: list_content,
    get_content: get_content,
    search_content: search_content,
    write_content: write_content,
  )
}
