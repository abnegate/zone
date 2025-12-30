import controllers/models
import controllers/static
import database/connection as db_connection
import middleware/auth
import routes/chats as chats_routes
import wisp.{type Request, type Response}

/// Main request router
/// Handles all HTTP requests (WebSocket is handled separately in manager.gleam)
pub fn handle_request(req: Request) -> Response {
  // SECURITY MODEL for Manager API:
  // - Primary protection: API key authentication (Bearer token or X-API-Key header)
  // - Secondary protection: Network isolation (accessible only via voiz_edge network)
  // - Tertiary protection: Path traversal validation on static file serving
  //
  // All API endpoints require valid API key authentication.
  // The index page injects the API key for frontend use.
  // Static files (CSS/JS) are served without auth.

  use <- wisp.log_request(req)
  use <- wisp.rescue_crashes
  use req <- wisp.handle_head(req)

  case wisp.path_segments(req) {
    // Index page - no auth (frontend handles login UI)
    [] -> static.serve_index()

    // Static files - no auth (CSS/JS are public)
    ["static", ..rest] -> static.serve_static(rest)

    // API endpoints - all require API key auth
    ["api", ..rest] -> {
      use <- auth.require_auth(req)
      route_api(req, rest)
    }

    _ -> wisp.not_found()
  }
}

/// Route API requests to appropriate controllers
fn route_api(req: Request, path: List(String)) -> Response {
  case path {
    // Models API
    ["models"] -> models.handle_models(req)
    ["models", author, model_name] -> models.handle_model_by_name(req, author <> "/" <> model_name)
    ["models", model_name] -> models.handle_model_by_name(req, model_name)

    // Chats API
    ["chats", ..rest] -> {
      let db = get_db_connection()
      chats_routes.handle_chats_route(req, rest, db)
    }

    _ -> wisp.not_found()
  }
}

/// Get database connection (lazy initialization)
fn get_db_connection() -> db_connection.Connection {
  case db_connection.connect() {
    Ok(conn) -> conn
    Error(_) -> {
      // Return a dummy connection for now (DB is stubbed)
      db_connection.Connection(db_connection.DbConfig(
        host: "",
        database: "",
        user: "",
        password: "",
      ))
    }
  }
}
