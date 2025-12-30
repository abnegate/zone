import gleam/list
import gleam/string
import simplifile
import wisp.{type Response}

/// Serve the index.html file
pub fn serve_index() -> Response {
  case simplifile.read("templates/index.html") {
    Ok(content) ->
      wisp.response(200)
      |> wisp.set_header("content-type", "text/html; charset=utf-8")
      |> wisp.string_body(content)
    Error(_) -> wisp.not_found()
  }
}

/// Serve static files from the static directory
pub fn serve_static(path: List(String)) -> Response {
  // Validate path segments to prevent directory traversal attacks
  case validate_path_segments(path) {
    False -> wisp.bad_request("Invalid path")
    True -> {
      let file_path = "static/" <> string.join(path, "/")

      case simplifile.read(file_path) {
        Ok(content) -> {
          let content_type = get_content_type(file_path)

          wisp.response(200)
          |> wisp.set_header("content-type", content_type)
          |> wisp.string_body(content)
        }
        Error(_) -> wisp.not_found()
      }
    }
  }
}

/// Validate path segments to prevent directory traversal attacks
pub fn validate_path_segments(segments: List(String)) -> Bool {
  // Reject any segment containing "..", ".", or starting with "/"
  // This prevents path traversal attacks like ../../../etc/passwd
  list.all(segments, fn(segment) {
    !string.contains(segment, "..") &&
    segment != "." &&
    segment != "" &&
    !string.starts_with(segment, "/")
  })
}

/// Get content type based on file extension
pub fn get_content_type(path: String) -> String {
  let types = [
    #(".css", "text/css; charset=utf-8"),
    #(".js", "application/javascript; charset=utf-8"),
    #(".json", "application/json"),
    #(".svg", "image/svg+xml"),
    #(".png", "image/png"),
    #(".jpg", "image/jpeg"),
    #(".jpeg", "image/jpeg"),
    #(".gif", "image/gif"),
    #(".ico", "image/x-icon"),
    #(".woff", "font/woff"),
    #(".woff2", "font/woff2"),
    #(".ttf", "font/ttf"),
    #(".html", "text/html; charset=utf-8"),
  ]

  case list.find(types, fn(t) { string.ends_with(path, t.0) }) {
    Ok(t) -> t.1
    Error(_) -> "text/plain"
  }
}
