/// File Source Interface
/// Abstraction for reading/writing files from different sources (GitHub, GitLab, filesystem)
import agents/file_source/filesystem
import agents/file_source/github
import agents/file_source/gitlab
import agents/file_source/types
import gleam/option.{type Option}
import models/source.{
  type FileContent, type FileEntry, type Source, type WriteResult,
  DiscordSourceConfig, FilesystemSourceConfig, GitHubSourceConfig,
  GitLabSourceConfig, ICalSourceConfig, IMAPSourceConfig, SlackSourceConfig,
  TextSourceConfig, WebSourceConfig,
}

// Re-export error type and helper
pub type FileSourceError =
  types.FileSourceError

pub const error_to_string = types.error_to_string

/// List files in a directory
pub fn list_files(
  source: Source,
  path: String,
) -> Result(List(FileEntry), FileSourceError) {
  case source.config {
    GitHubSourceConfig(cfg) -> github.list_files(source.credentials, cfg, path)
    GitLabSourceConfig(cfg) -> gitlab.list_files(source.credentials, cfg, path)
    FilesystemSourceConfig(cfg) -> filesystem.list_files(cfg, path)
    ICalSourceConfig(_)
    | IMAPSourceConfig(_)
    | DiscordSourceConfig(_)
    | SlackSourceConfig(_)
    | WebSourceConfig(_)
    | TextSourceConfig(_) ->
      Error(types.UnsupportedOperation(
        "list_files not supported for this source type",
      ))
  }
}

/// Read file content
pub fn read_file(
  source: Source,
  path: String,
) -> Result(FileContent, FileSourceError) {
  case source.config {
    GitHubSourceConfig(cfg) -> github.read_file(source.credentials, cfg, path)
    GitLabSourceConfig(cfg) -> gitlab.read_file(source.credentials, cfg, path)
    FilesystemSourceConfig(cfg) -> filesystem.read_file(cfg, path)
    ICalSourceConfig(_)
    | IMAPSourceConfig(_)
    | DiscordSourceConfig(_)
    | SlackSourceConfig(_)
    | WebSourceConfig(_)
    | TextSourceConfig(_) ->
      Error(types.UnsupportedOperation(
        "read_file not supported for this source type",
      ))
  }
}

/// Write file content
pub fn write_file(
  source: Source,
  path: String,
  content: String,
  message: String,
) -> Result(WriteResult, FileSourceError) {
  case source.config {
    GitHubSourceConfig(cfg) ->
      github.write_file(source.credentials, cfg, path, content, message)
    GitLabSourceConfig(cfg) ->
      gitlab.write_file(source.credentials, cfg, path, content, message)
    FilesystemSourceConfig(cfg) -> filesystem.write_file(cfg, path, content)
    ICalSourceConfig(_)
    | IMAPSourceConfig(_)
    | DiscordSourceConfig(_)
    | SlackSourceConfig(_)
    | WebSourceConfig(_)
    | TextSourceConfig(_) ->
      Error(types.UnsupportedOperation(
        "write_file not supported for this source type",
      ))
  }
}

/// Search for files matching a pattern
pub fn search_files(
  source: Source,
  query: String,
  path: Option(String),
) -> Result(List(#(String, String)), FileSourceError) {
  case source.config {
    GitHubSourceConfig(cfg) ->
      github.search_code(source.credentials, cfg, query)
    GitLabSourceConfig(cfg) ->
      gitlab.search_code(source.credentials, cfg, query, path)
    FilesystemSourceConfig(cfg) -> filesystem.search(cfg, query, path)
    ICalSourceConfig(_)
    | IMAPSourceConfig(_)
    | DiscordSourceConfig(_)
    | SlackSourceConfig(_)
    | WebSourceConfig(_)
    | TextSourceConfig(_) ->
      Error(types.UnsupportedOperation(
        "search_files not supported for this source type",
      ))
  }
}
