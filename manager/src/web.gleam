/// Web module - Wisp best practices implementation
///
/// This module provides:
/// - Context type for dependency injection
/// - Centralized middleware stack
/// - JSON response helpers
import cache/connection as cache_connection
import database/connection.{type Connection}
import gleam/json
import wisp.{type Request, type Response}

// =============================================================================
// Context Type
// =============================================================================

/// Application context holding shared dependencies.
/// Passed through all request handlers for dependency injection.
pub type Context {
  Context(
    /// Database connection pool
    db: Connection,
    /// Cache connection (Valkey/Redis)
    cache: cache_connection.CacheConnection,
  )
}

// =============================================================================
// Middleware Stack
// =============================================================================

/// Centralized middleware that applies Wisp best practices:
/// - Method override for HTML form support (PUT/PATCH/DELETE via POST)
/// - Request logging
/// - Crash recovery
/// - HEAD request handling
/// - CSRF protection via known headers
pub fn middleware(
  req: Request,
  _ctx: Context,
  handler: fn(Request) -> Response,
) -> Response {
  // Method override allows HTML forms to use PUT/PATCH/DELETE
  // by setting _method in form data or query string
  let req = wisp.method_override(req)

  // Log all requests
  use <- wisp.log_request(req)

  // Recover from crashes gracefully
  use <- wisp.rescue_crashes

  // Handle HEAD requests by running GET and stripping the body
  use req <- wisp.handle_head(req)

  // CSRF protection: require requests with bodies to have
  // known headers that browsers won't send cross-origin
  use req <- wisp.csrf_known_header_protection(req)

  handler(req)
}

// =============================================================================
// JSON Response Helpers
// =============================================================================

/// Create a successful JSON response with data
pub fn json_response(status: Int, data: json.Json) -> Response {
  let body = json.to_string(data)

  wisp.response(status)
  |> wisp.set_header("content-type", "application/json")
  |> wisp.string_body(body)
}

/// Create a success response wrapping data in {success: true, ...}
pub fn json_success(data: List(#(String, json.Json))) -> Response {
  json_response(200, json.object([#("success", json.bool(True)), ..data]))
}

/// Create a success response for resource creation (201)
pub fn json_created(data: List(#(String, json.Json))) -> Response {
  json_response(201, json.object([#("success", json.bool(True)), ..data]))
}

/// Create an error response
pub fn json_error(status: Int, message: String) -> Response {
  json_response(
    status,
    json.object([
      #("success", json.bool(False)),
      #("error", json.string(message)),
    ]),
  )
}

/// Common error responses
pub fn bad_request(message: String) -> Response {
  json_error(400, message)
}

pub fn unauthorized(message: String) -> Response {
  json_error(401, message)
}

pub fn not_found(message: String) -> Response {
  json_error(404, message)
}

pub fn internal_error(message: String) -> Response {
  json_error(500, message)
}
