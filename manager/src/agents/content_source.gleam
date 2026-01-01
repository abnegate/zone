/// Content Source Interface
/// Unified abstraction for accessing content from different source types
///
/// To add a new source type:
/// 1. Create a new module in agents/content_source/ (e.g., mysource.gleam)
/// 2. Implement list_content, get_content, search_content, write_content
/// 3. Export a handler() function that returns a SourceHandler
/// 4. Add the SourceConfig pattern to get_handler() below
/// 5. Add the SourceType to models/source.gleam
import agents/content_source/calendar
import agents/content_source/chat
import agents/content_source/file
import agents/content_source/mail
import agents/content_source/text
import agents/content_source/types.{type SourceHandler}
import agents/content_source/web
import models/content.{
  type ContentItem, type ListQuery, type ListResult, type SearchQuery,
  type WriteResult,
}
import models/source.{
  type Source, DiscordSourceConfig, FilesystemSourceConfig, GitHubSourceConfig,
  GitLabSourceConfig, ICalSourceConfig, IMAPSourceConfig, SlackSourceConfig,
  TextSourceConfig, WebSourceConfig,
}

// Re-export error type
pub type ContentSourceError =
  types.ContentSourceError

pub const error_to_string = types.error_to_string

/// Get the appropriate handler for a source based on its config
/// This is the ONLY place where source types are dispatched
fn get_handler(source: Source) -> SourceHandler {
  case source.config {
    // File sources (GitHub, GitLab, Filesystem)
    GitHubSourceConfig(_) | GitLabSourceConfig(_) | FilesystemSourceConfig(_) ->
      file.handler()
    // Calendar sources
    ICalSourceConfig(_) -> calendar.handler()
    // Mail sources
    IMAPSourceConfig(_) -> mail.handler()
    // Chat sources (Discord, Slack)
    DiscordSourceConfig(_) | SlackSourceConfig(_) -> chat.handler()
    // Web sources
    WebSourceConfig(_) -> web.handler()
    // Text sources
    TextSourceConfig(_) -> text.handler()
  }
}

/// List content from a source
pub fn list_content(
  source: Source,
  query: ListQuery,
) -> Result(ListResult, ContentSourceError) {
  let handler = get_handler(source)
  handler.list_content(source, query)
}

/// Get a specific content item by ID
pub fn get_content(
  source: Source,
  item_id: String,
) -> Result(ContentItem, ContentSourceError) {
  let handler = get_handler(source)
  handler.get_content(source, item_id)
}

/// Search content within a source
pub fn search_content(
  source: Source,
  query: SearchQuery,
) -> Result(List(ContentItem), ContentSourceError) {
  let handler = get_handler(source)
  handler.search_content(source, query)
}

/// Write/update content to a source
pub fn write_content(
  source: Source,
  item: ContentItem,
) -> Result(WriteResult, ContentSourceError) {
  let handler = get_handler(source)
  handler.write_content(source, item)
}
