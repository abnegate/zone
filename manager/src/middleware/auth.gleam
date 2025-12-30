import config
import gleam/http/request
import gleam/json
import gleam/string
import wisp.{type Request, type Response}

/// Validate API key from request headers
/// Checks both Authorization: Bearer <key> and X-API-Key headers
pub fn validate_api_key(req: Request) -> Bool {
  let expected_key = config.get_manager_api_key()

  // If no API key is configured, reject all requests
  case expected_key {
    "" -> False
    key -> {
      // Check Authorization: Bearer <key> header
      let bearer_valid = case request.get_header(req, "authorization") {
        Ok(auth) -> {
          case string.starts_with(auth, "Bearer ") {
            True -> string.drop_start(auth, 7) == key
            False -> False
          }
        }
        Error(_) -> False
      }

      // Check X-API-Key header as fallback
      let api_key_valid = case request.get_header(req, "x-api-key") {
        Ok(provided_key) -> provided_key == key
        Error(_) -> False
      }

      bearer_valid || api_key_valid
    }
  }
}

/// Return 401 Unauthorized response
pub fn unauthorized_response() -> Response {
  let error_json =
    json.object([
      #("success", json.bool(False)),
      #("error", json.string("Unauthorized: Invalid or missing API key")),
    ])
    |> json.to_string

  wisp.response(401)
  |> wisp.set_header("content-type", "application/json")
  |> wisp.set_header("www-authenticate", "Bearer")
  |> wisp.string_body(error_json)
}

/// Middleware that requires API key authentication
/// Usage: use <- require_auth(req)
pub fn require_auth(
  req: Request,
  handler: fn() -> Response,
) -> Response {
  case validate_api_key(req) {
    True -> handler()
    False -> unauthorized_response()
  }
}
