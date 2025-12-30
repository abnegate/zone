import gleam/http
import gleam/http/request
import gleam/httpc
import gleam/list

/// Make a GET request to a URL
pub fn get(
  url: String,
  headers: List(#(String, String)),
) -> Result(String, String) {
  case request.to(url) {
    Ok(req) -> {
      let req_with_headers =
        list.fold(headers, req, fn(r, h) { request.set_header(r, h.0, h.1) })

      case httpc.send(req_with_headers) {
        Ok(response) -> Ok(response.body)
        Error(_) -> Error("HTTP request failed")
      }
    }
    Error(_) -> Error("Invalid URL: " <> url)
  }
}

/// Make a POST request to a URL
pub fn post(
  url: String,
  body: String,
  headers: List(#(String, String)),
) -> Result(String, String) {
  case request.to(url) {
    Ok(req) -> {
      let req_with_method = request.set_method(req, http.Post)
      let req_with_body = request.set_body(req_with_method, body)
      let req_with_content_type =
        request.set_header(req_with_body, "content-type", "application/json")
      let req_with_headers =
        list.fold(headers, req_with_content_type, fn(r, h) {
          request.set_header(r, h.0, h.1)
        })

      case httpc.send(req_with_headers) {
        Ok(response) -> Ok(response.body)
        Error(_) -> Error("HTTP request failed")
      }
    }
    Error(_) -> Error("Invalid URL: " <> url)
  }
}

/// Make a DELETE request to a URL
pub fn delete(
  url: String,
  body: String,
  headers: List(#(String, String)),
) -> Result(String, String) {
  case request.to(url) {
    Ok(req) -> {
      let req_with_method = request.set_method(req, http.Delete)
      let req_with_body = request.set_body(req_with_method, body)
      let req_with_content_type =
        request.set_header(req_with_body, "content-type", "application/json")
      let req_with_headers =
        list.fold(headers, req_with_content_type, fn(r, h) {
          request.set_header(r, h.0, h.1)
        })

      case httpc.send(req_with_headers) {
        Ok(response) -> Ok(response.body)
        Error(_) -> Error("HTTP request failed")
      }
    }
    Error(_) -> Error("Invalid URL: " <> url)
  }
}
