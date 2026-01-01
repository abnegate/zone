import auth/jwt.{type JwtClaims}
import birl
import config
import gleam/http
import gleam/http/request
import gleam/json
import gleam/list
import gleam/string
import wisp.{type Request, type Response}

/// Extract JWT claims from request
pub fn get_jwt_claims(req: Request) -> Result(JwtClaims, String) {
  let jwt_secret = config.get_jwt_secret()

  case request.get_header(req, "authorization") {
    Ok(auth_header) -> {
      case string.starts_with(auth_header, "Bearer ") {
        True -> {
          let token = string.drop_start(auth_header, 7)
          jwt.validate_token(token, jwt_secret)
        }
        False -> Error("Invalid authorization header format")
      }
    }
    Error(_) -> Error("No authorization header")
  }
}

/// Middleware that requires JWT authentication
/// Usage: use claims <- require_jwt_auth(req)
pub fn require_jwt_auth(
  req: Request,
  handler: fn(JwtClaims) -> Response,
) -> Response {
  case get_jwt_claims(req) {
    Ok(claims) -> handler(claims)
    Error(err) -> unauthorized_response(err)
  }
}

/// Middleware that requires a specific permission
/// Usage: use claims <- require_permission(req, "chats:create")
pub fn require_permission(
  req: Request,
  permission: String,
  handler: fn(JwtClaims) -> Response,
) -> Response {
  use claims <- require_jwt_auth(req)

  case jwt.has_permission(claims, permission) {
    True -> handler(claims)
    False -> forbidden_response("Requires permission: " <> permission)
  }
}

/// Middleware that requires any of the given permissions
pub fn require_any_permission(
  req: Request,
  permissions: List(String),
  handler: fn(JwtClaims) -> Response,
) -> Response {
  use claims <- require_jwt_auth(req)

  case jwt.has_any_permission(claims, permissions) {
    True -> handler(claims)
    False ->
      forbidden_response("Requires one of: " <> string.join(permissions, ", "))
  }
}

/// Get the permission required for a resource based on HTTP method
pub fn permission_for_method(resource: String, method: http.Method) -> String {
  let action = case method {
    http.Get | http.Head -> "read"
    http.Post -> "create"
    http.Put | http.Patch -> "update"
    http.Delete -> "delete"
    _ -> "read"
  }
  resource <> ":" <> action
}

/// Return 401 Unauthorized response
pub fn unauthorized_response(message: String) -> Response {
  let error_json =
    json.object([
      #("success", json.bool(False)),
      #("error", json.string("Unauthorized: " <> message)),
    ])
    |> json.to_string

  wisp.response(401)
  |> wisp.set_header("content-type", "application/json")
  |> wisp.set_header("www-authenticate", "Bearer")
  |> wisp.string_body(error_json)
}

/// Return 403 Forbidden response
pub fn forbidden_response(message: String) -> Response {
  let error_json =
    json.object([
      #("success", json.bool(False)),
      #("error", json.string("Forbidden: " <> message)),
    ])
    |> json.to_string

  wisp.response(403)
  |> wisp.set_header("content-type", "application/json")
  |> wisp.string_body(error_json)
}

// --- Legacy API key auth (kept for backward compatibility) ---

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

/// Legacy middleware that requires API key authentication
/// Usage: use <- require_auth(req)
pub fn require_auth(req: Request, handler: fn() -> Response) -> Response {
  case validate_api_key(req) {
    True -> handler()
    False -> unauthorized_response("Invalid or missing API key")
  }
}
