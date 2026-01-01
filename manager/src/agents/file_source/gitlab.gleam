/// GitLab File Source Provider
/// Implementation for reading/writing files via GitLab API
import agents/file_source/types.{
  type FileSourceError, AccessDenied, NetworkError, NotFound, ParseError,
  base64_decode, uri_encode,
}
import gleam/dynamic/decode
import gleam/http
import gleam/http/request
import gleam/httpc
import gleam/int
import gleam/json
import gleam/option.{type Option, None, Some}
import models/source.{
  type FileContent, type FileEntry, type GitLabConfig, type WriteResult,
  FileContent, FileEntry, WriteResult,
}

/// List files in a GitLab repository directory
pub fn list_files(
  token: Option(String),
  cfg: GitLabConfig,
  path: String,
) -> Result(List(FileEntry), FileSourceError) {
  let full_path = case cfg.base_path, path {
    "", p -> p
    base, "" -> base
    base, p -> base <> "/" <> p
  }

  let encoded_path = uri_encode(full_path)
  let encoded_project = uri_encode(cfg.project_id)

  let url =
    cfg.host
    <> "/api/v4/projects/"
    <> encoded_project
    <> "/repository/tree?path="
    <> encoded_path
    <> "&ref="
    <> cfg.branch

  case make_request(token, url) {
    Ok(body) -> parse_tree(body)
    Error(e) -> Error(e)
  }
}

/// Read a file from GitLab repository
pub fn read_file(
  token: Option(String),
  cfg: GitLabConfig,
  path: String,
) -> Result(FileContent, FileSourceError) {
  let full_path = case cfg.base_path {
    "" -> path
    base -> base <> "/" <> path
  }

  let encoded_path = uri_encode(full_path)
  let encoded_project = uri_encode(cfg.project_id)

  let url =
    cfg.host
    <> "/api/v4/projects/"
    <> encoded_project
    <> "/repository/files/"
    <> encoded_path
    <> "?ref="
    <> cfg.branch

  case make_request(token, url) {
    Ok(body) -> parse_file_content(body, path)
    Error(e) -> Error(e)
  }
}

/// Write a file to GitLab repository
pub fn write_file(
  token: Option(String),
  cfg: GitLabConfig,
  path: String,
  content: String,
  message: String,
) -> Result(WriteResult, FileSourceError) {
  case token {
    None -> Error(AccessDenied("GitLab token required for write operations"))
    Some(t) -> {
      let full_path = case cfg.base_path {
        "" -> path
        base -> base <> "/" <> path
      }

      let encoded_path = uri_encode(full_path)
      let encoded_project = uri_encode(cfg.project_id)

      // Check if file exists
      let check_url =
        cfg.host
        <> "/api/v4/projects/"
        <> encoded_project
        <> "/repository/files/"
        <> encoded_path
        <> "?ref="
        <> cfg.branch

      let method = case make_request(Some(t), check_url) {
        Ok(_) -> http.Put
        Error(_) -> http.Post
      }

      let url =
        cfg.host
        <> "/api/v4/projects/"
        <> encoded_project
        <> "/repository/files/"
        <> encoded_path

      let body =
        json.object([
          #("branch", json.string(cfg.branch)),
          #("content", json.string(content)),
          #("commit_message", json.string(message)),
        ])
        |> json.to_string

      case make_write_request(t, url, body, method) {
        Ok(_) -> Ok(WriteResult(path, None, "File updated"))
        Error(e) -> Error(e)
      }
    }
  }
}

/// Search for code in GitLab repository
pub fn search_code(
  token: Option(String),
  cfg: GitLabConfig,
  query: String,
  _path: Option(String),
) -> Result(List(#(String, String)), FileSourceError) {
  let encoded_project = uri_encode(cfg.project_id)
  let url =
    cfg.host
    <> "/api/v4/projects/"
    <> encoded_project
    <> "/search?scope=blobs&search="
    <> uri_encode(query)

  case make_request(token, url) {
    Ok(body) -> parse_search_results(body)
    Error(e) -> Error(e)
  }
}

// =============================================================================
// HTTP Request Helpers
// =============================================================================

fn make_request(
  token: Option(String),
  url: String,
) -> Result(String, FileSourceError) {
  case request.to(url) {
    Ok(req) -> {
      let req =
        req
        |> request.set_method(http.Get)
        |> request.set_header("accept", "application/json")

      let req = case token {
        Some(t) -> request.set_header(req, "private-token", t)
        None -> req
      }

      case httpc.send(req) {
        Ok(resp) -> {
          case resp.status {
            200 -> Ok(resp.body)
            404 -> Error(NotFound(url))
            401 | 403 -> Error(AccessDenied("GitLab API access denied"))
            status ->
              Error(NetworkError(
                "GitLab API returned " <> int.to_string(status),
              ))
          }
        }
        Error(_) -> Error(NetworkError("Failed to connect to GitLab API"))
      }
    }
    Error(_) -> Error(NetworkError("Invalid GitLab API URL"))
  }
}

fn make_write_request(
  token: String,
  url: String,
  body: String,
  method: http.Method,
) -> Result(String, FileSourceError) {
  case request.to(url) {
    Ok(req) -> {
      let req =
        req
        |> request.set_method(method)
        |> request.set_body(body)
        |> request.set_header("private-token", token)
        |> request.set_header("content-type", "application/json")

      case httpc.send(req) {
        Ok(resp) -> {
          case resp.status {
            200 | 201 -> Ok(resp.body)
            404 -> Error(NotFound(url))
            401 | 403 -> Error(AccessDenied("GitLab write access denied"))
            status ->
              Error(NetworkError(
                "GitLab API returned " <> int.to_string(status),
              ))
          }
        }
        Error(_) -> Error(NetworkError("Failed to connect to GitLab API"))
      }
    }
    Error(_) -> Error(NetworkError("Invalid GitLab API URL"))
  }
}

// =============================================================================
// Response Parsing
// =============================================================================

fn parse_tree(body: String) -> Result(List(FileEntry), FileSourceError) {
  let decoder =
    decode.list({
      use name <- decode.field("name", decode.string)
      use path <- decode.field("path", decode.string)
      use type_ <- decode.field("type", decode.string)
      decode.success(FileEntry(
        path: path,
        name: name,
        is_directory: type_ == "tree",
        size: None,
        sha: None,
      ))
    })

  case json.parse(body, decoder) {
    Ok(entries) -> Ok(entries)
    Error(_) -> Error(ParseError("Failed to parse GitLab tree response"))
  }
}

fn parse_file_content(
  body: String,
  path: String,
) -> Result(FileContent, FileSourceError) {
  let decoder = {
    use content <- decode.field("content", decode.string)
    use encoding <- decode.optional_field("encoding", "base64", decode.string)
    decode.success(#(content, encoding))
  }

  case json.parse(body, decoder) {
    Ok(#(content, encoding)) -> {
      let decoded_content = case encoding {
        "base64" -> base64_decode(content)
        _ -> content
      }
      Ok(FileContent(path, decoded_content, None, encoding))
    }
    Error(_) -> Error(ParseError("Failed to parse GitLab file content"))
  }
}

fn parse_search_results(
  body: String,
) -> Result(List(#(String, String)), FileSourceError) {
  let decoder =
    decode.list({
      use path <- decode.field("path", decode.string)
      use filename <- decode.field("filename", decode.string)
      decode.success(#(path, filename))
    })

  case json.parse(body, decoder) {
    Ok(items) -> Ok(items)
    Error(_) -> Error(ParseError("Failed to parse GitLab search results"))
  }
}
