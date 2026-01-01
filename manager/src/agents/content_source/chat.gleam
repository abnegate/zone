/// Chat content source (stub)
/// Placeholder for Discord, Slack, and other chat sources
import agents/content_source/types.{
  type ContentSourceError, type SourceHandler, SourceHandler,
  UnsupportedOperation,
}
import models/content.{
  type ContentItem, type ListQuery, type ListResult, type SearchQuery,
  type WriteResult,
}
import models/source.{type Source}

/// List content - not implemented
pub fn list_content(
  _source: Source,
  _query: ListQuery,
) -> Result(ListResult, ContentSourceError) {
  Error(UnsupportedOperation("Chat sources are not yet implemented"))
}

/// Get content - not implemented
pub fn get_content(
  _source: Source,
  _item_id: String,
) -> Result(ContentItem, ContentSourceError) {
  Error(UnsupportedOperation("Chat sources are not yet implemented"))
}

/// Search content - not implemented
pub fn search_content(
  _source: Source,
  _query: SearchQuery,
) -> Result(List(ContentItem), ContentSourceError) {
  Error(UnsupportedOperation("Chat sources are not yet implemented"))
}

/// Write content - not implemented
pub fn write_content(
  _source: Source,
  _item: ContentItem,
) -> Result(WriteResult, ContentSourceError) {
  Error(UnsupportedOperation("Chat sources are not yet implemented"))
}

/// Get the handler for chat sources (stub)
pub fn handler() -> SourceHandler {
  SourceHandler(
    list_content: list_content,
    get_content: get_content,
    search_content: search_content,
    write_content: write_content,
  )
}
