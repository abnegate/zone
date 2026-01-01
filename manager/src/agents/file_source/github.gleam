/// GitHub File Source Provider
/// Implementation for reading/writing files via GitHub API
import agents/file_source/types.{
  type FileSourceError, AccessDenied, NetworkError, NotFound, ParseError,
  WriteError, base64_decode, base64_encode, uri_encode,
}
import gleam/dynamic/decode
import gleam/http
import gleam/http/request
import gleam/httpc
import gleam/int
import gleam/json
import gleam/option.{type Option, None, Some}
import gleam/string
import models/source.{
  type FileContent, type FileEntry, type GitHubConfig, type WriteResult,
  FileContent, FileEntry, WriteResult,
}

/// List files in a GitHub repository directory
pub fn list_files(
  token: Option(String),
  cfg: GitHubConfig,
  path: String,
) -> Result(List(FileEntry), FileSourceError) {
  let full_path = case cfg.base_path, path {
    "", p -> p
    base, "" -> base
    base, p -> base <> "/" <> p
  }

  let url =
    "https://api.github.com/repos/"
    <> cfg.owner
    <> "/"
    <> cfg.repo
    <> "/contents/"
    <> full_path
    <> "?ref="
    <> cfg.branch

  case make_request(token, url) {
    Ok(body) -> parse_contents(body)
    Error(e) -> Error(e)
  }
}

/// Read a file from GitHub repository
pub fn read_file(
  token: Option(String),
  cfg: GitHubConfig,
  path: String,
) -> Result(FileContent, FileSourceError) {
  let full_path = case cfg.base_path {
    "" -> path
    base -> base <> "/" <> path
  }

  let url =
    "https://api.github.com/repos/"
    <> cfg.owner
    <> "/"
    <> cfg.repo
    <> "/contents/"
    <> full_path
    <> "?ref="
    <> cfg.branch

  case make_request(token, url) {
    Ok(body) -> parse_file_content(body, path)
    Error(e) -> Error(e)
  }
}

/// Write a file to GitHub repository
pub fn write_file(
  token: Option(String),
  cfg: GitHubConfig,
  path: String,
  content: String,
  message: String,
) -> Result(WriteResult, FileSourceError) {
  case token {
    None -> Error(AccessDenied("GitHub token required for write operations"))
    Some(t) -> {
      let full_path = case cfg.base_path {
        "" -> path
        base -> base <> "/" <> path
      }

      let url =
        "https://api.github.com/repos/"
        <> cfg.owner
        <> "/"
        <> cfg.repo
        <> "/contents/"
        <> full_path

      // First, try to get existing file SHA
      let existing_sha = case
        make_request(Some(t), url <> "?ref=" <> cfg.branch)
      {
        Ok(body) -> extract_sha_from_content(body)
        Error(_) -> None
      }

      // Prepare request body
      let body_obj = [
        #("message", json.string(message)),
        #("content", json.string(base64_encode(content))),
        #("branch", json.string(cfg.branch)),
      ]

      let body_with_sha = case existing_sha {
        Some(sha) -> [#("sha", json.string(sha)), ..body_obj]
        None -> body_obj
      }

      let body = json.object(body_with_sha) |> json.to_string

      case make_put_request(t, url, body) {
        Ok(resp_body) -> {
          case extract_sha_from_commit(resp_body) {
            Some(sha) -> Ok(WriteResult(path, Some(sha), "File updated"))
            None -> Ok(WriteResult(path, None, "File updated"))
          }
        }
        Error(e) -> Error(e)
      }
    }
  }
}

/// Search for code in GitHub repository
pub fn search_code(
  token: Option(String),
  cfg: GitHubConfig,
  query: String,
) -> Result(List(#(String, String)), FileSourceError) {
  let search_query = query <> "+repo:" <> cfg.owner <> "/" <> cfg.repo

  let url =
    "https://api.github.com/search/code?q="
    <> uri_encode(search_query)
    <> "&per_page=20"

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
        |> request.set_header("accept", "application/vnd.github.v3+json")
        |> request.set_header("user-agent", "zone-manager")

      let req = case token {
        Some(t) -> request.set_header(req, "authorization", "Bearer " <> t)
        None -> req
      }

      case httpc.send(req) {
        Ok(resp) -> {
          case resp.status {
            200 -> Ok(resp.body)
            404 -> Error(NotFound(url))
            401 | 403 -> Error(AccessDenied("GitHub API access denied"))
            status ->
              Error(NetworkError(
                "GitHub API returned " <> int.to_string(status),
              ))
          }
        }
        Error(_) -> Error(NetworkError("Failed to connect to GitHub API"))
      }
    }
    Error(_) -> Error(NetworkError("Invalid GitHub API URL"))
  }
}

fn make_put_request(
  token: String,
  url: String,
  body: String,
) -> Result(String, FileSourceError) {
  case request.to(url) {
    Ok(req) -> {
      let req =
        req
        |> request.set_method(http.Put)
        |> request.set_body(body)
        |> request.set_header("accept", "application/vnd.github.v3+json")
        |> request.set_header("authorization", "Bearer " <> token)
        |> request.set_header("content-type", "application/json")
        |> request.set_header("user-agent", "zone-manager")

      case httpc.send(req) {
        Ok(resp) -> {
          case resp.status {
            200 | 201 -> Ok(resp.body)
            404 -> Error(NotFound(url))
            401 | 403 -> Error(AccessDenied("GitHub write access denied"))
            422 -> Error(WriteError("Invalid request: " <> resp.body))
            status ->
              Error(NetworkError(
                "GitHub API returned " <> int.to_string(status),
              ))
          }
        }
        Error(_) -> Error(NetworkError("Failed to connect to GitHub API"))
      }
    }
    Error(_) -> Error(NetworkError("Invalid GitHub API URL"))
  }
}

// =============================================================================
// Response Parsing
// =============================================================================

fn parse_contents(body: String) -> Result(List(FileEntry), FileSourceError) {
  let decoder =
    decode.list({
      use name <- decode.field("name", decode.string)
      use path <- decode.field("path", decode.string)
      use type_ <- decode.field("type", decode.string)
      use size <- decode.optional_field(
        "size",
        None,
        decode.optional(decode.int),
      )
      use sha <- decode.optional_field(
        "sha",
        None,
        decode.optional(decode.string),
      )
      decode.success(FileEntry(
        path: path,
        name: name,
        is_directory: type_ == "dir",
        size: size,
        sha: sha,
      ))
    })

  case json.parse(body, decoder) {
    Ok(entries) -> Ok(entries)
    Error(_) -> Error(ParseError("Failed to parse GitHub contents response"))
  }
}

fn parse_file_content(
  body: String,
  path: String,
) -> Result(FileContent, FileSourceError) {
  let decoder = {
    use content <- decode.field("content", decode.string)
    use sha <- decode.optional_field(
      "sha",
      None,
      decode.optional(decode.string),
    )
    use encoding <- decode.optional_field(
      "encoding",
      Some("base64"),
      decode.optional(decode.string),
    )
    decode.success(#(content, sha, encoding))
  }

  case json.parse(body, decoder) {
    Ok(#(content, sha, enc_opt)) -> {
      let encoding = option.unwrap(enc_opt, "base64")
      let decoded_content = case encoding {
        "base64" -> base64_decode(string.replace(content, "\n", ""))
        _ -> content
      }
      Ok(FileContent(path, decoded_content, sha, encoding))
    }
    Error(_) -> Error(ParseError("Failed to parse GitHub file content"))
  }
}

fn parse_search_results(
  body: String,
) -> Result(List(#(String, String)), FileSourceError) {
  let decoder = {
    use items <- decode.field(
      "items",
      decode.list({
        use path <- decode.field("path", decode.string)
        use name <- decode.field("name", decode.string)
        decode.success(#(path, name))
      }),
    )
    decode.success(items)
  }

  case json.parse(body, decoder) {
    Ok(items) -> Ok(items)
    Error(_) -> Error(ParseError("Failed to parse GitHub search results"))
  }
}

fn extract_sha_from_content(body: String) -> Option(String) {
  let decoder = {
    use sha <- decode.field("sha", decode.string)
    decode.success(sha)
  }
  case json.parse(body, decoder) {
    Ok(sha) -> Some(sha)
    Error(_) -> None
  }
}

fn extract_sha_from_commit(body: String) -> Option(String) {
  let decoder = {
    use content <- decode.field("content", {
      use sha <- decode.field("sha", decode.string)
      decode.success(sha)
    })
    decode.success(content)
  }
  case json.parse(body, decoder) {
    Ok(sha) -> Some(sha)
    Error(_) -> None
  }
}
