/// Filesystem File Source Provider
/// Implementation for reading/writing files from local filesystem
import agents/file_source/types.{
  type FileSourceError, AccessDenied, NotFound, WriteError,
}
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/string
import models/source.{
  type FileContent, type FileEntry, type FilesystemConfig, type WriteResult,
  FileContent, FileEntry, WriteResult,
}
import simplifile

/// List files in a filesystem directory
pub fn list_files(
  cfg: FilesystemConfig,
  path: String,
) -> Result(List(FileEntry), FileSourceError) {
  let full_path = case path {
    "" -> cfg.base_path
    p -> cfg.base_path <> "/" <> p
  }

  case simplifile.read_directory(full_path) {
    Ok(entries) -> {
      let file_entries =
        list.filter_map(entries, fn(name) {
          let entry_path = full_path <> "/" <> name
          case simplifile.is_directory(entry_path) {
            Ok(is_dir) -> {
              let size = case is_dir {
                True -> None
                False -> {
                  case simplifile.file_info(entry_path) {
                    Ok(info) -> Some(info.size)
                    Error(_) -> None
                  }
                }
              }
              let relative_path = case path {
                "" -> name
                p -> p <> "/" <> name
              }
              Ok(FileEntry(
                path: relative_path,
                name: name,
                is_directory: is_dir,
                size: size,
                sha: None,
              ))
            }
            Error(_) -> Error(Nil)
          }
        })
      Ok(file_entries)
    }
    Error(_) -> Error(NotFound(full_path))
  }
}

/// Read a file from filesystem
pub fn read_file(
  cfg: FilesystemConfig,
  path: String,
) -> Result(FileContent, FileSourceError) {
  let full_path = cfg.base_path <> "/" <> path

  case simplifile.read(full_path) {
    Ok(content) -> Ok(FileContent(path, content, None, "utf-8"))
    Error(_) -> Error(NotFound(path))
  }
}

/// Write a file to filesystem
pub fn write_file(
  cfg: FilesystemConfig,
  path: String,
  content: String,
) -> Result(WriteResult, FileSourceError) {
  case cfg.allow_writes {
    False -> Error(AccessDenied("Write operations disabled for this source"))
    True -> {
      let full_path = cfg.base_path <> "/" <> path

      // Ensure parent directory exists
      let parent = get_parent_path(full_path)
      case simplifile.create_directory_all(parent) {
        Ok(_) | Error(_) -> Nil
      }

      case simplifile.write(full_path, content) {
        Ok(_) -> Ok(WriteResult(path, None, "File written"))
        Error(_) -> Error(WriteError("Failed to write file: " <> path))
      }
    }
  }
}

/// Search for files matching a pattern in filesystem
pub fn search(
  cfg: FilesystemConfig,
  query: String,
  path: Option(String),
) -> Result(List(#(String, String)), FileSourceError) {
  let search_path = case path {
    Some(p) -> cfg.base_path <> "/" <> p
    None -> cfg.base_path
  }

  case find_matching_files(search_path, query, cfg.base_path) {
    Ok(matches) -> Ok(matches)
    Error(_) -> Error(NotFound(search_path))
  }
}

// =============================================================================
// Helper Functions
// =============================================================================

fn find_matching_files(
  dir: String,
  query: String,
  base_path: String,
) -> Result(List(#(String, String)), Nil) {
  case simplifile.read_directory(dir) {
    Ok(entries) -> {
      let results =
        list.flat_map(entries, fn(name) {
          let full_path = dir <> "/" <> name
          case simplifile.is_directory(full_path) {
            Ok(True) -> {
              case find_matching_files(full_path, query, base_path) {
                Ok(sub_results) -> sub_results
                Error(_) -> []
              }
            }
            Ok(False) -> {
              // Check if file contains query
              case simplifile.read(full_path) {
                Ok(content) -> {
                  case string.contains(content, query) {
                    True -> {
                      let relative =
                        string.replace(full_path, base_path <> "/", "")
                      [#(relative, name)]
                    }
                    False -> []
                  }
                }
                Error(_) -> []
              }
            }
            Error(_) -> []
          }
        })
      Ok(list.take(results, 50))
    }
    Error(_) -> Error(Nil)
  }
}

fn get_parent_path(path: String) -> String {
  case string.split(path, "/") |> list.reverse {
    [_, ..rest] -> list.reverse(rest) |> string.join("/")
    _ -> path
  }
}
