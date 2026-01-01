import auth/jwt.{type JwtClaims}
import controllers/auth as auth_routes
import controllers/chats as chats_routes
import controllers/models
import controllers/organizations as organizations_routes
import controllers/projects as projects_routes
import controllers/sources as sources_routes
import controllers/tasks as tasks_routes
import middleware/auth
import middleware/metrics
import web.{type Context}
import wisp.{type Request, type Response}

/// Main request router
/// Handles all HTTP requests (WebSocket is handled separately in manager.gleam)
pub fn handle_request(req: Request, ctx: Context) -> Response {
  // Apply centralized middleware stack (logging, crash recovery, CSRF)
  use req <- web.middleware(req, ctx)

  case wisp.path_segments(req) {
    // Health check - no auth required
    ["api", "health"] -> wisp.ok()

    // Prometheus metrics endpoint - no auth required for scraping
    ["metrics"] -> {
      wisp.response(200)
      |> wisp.set_header("content-type", "text/plain; version=0.0.4; charset=utf-8")
      |> wisp.string_body(metrics.export())
    }

    // Auth routes - no auth required (login, register, refresh, logout)
    ["api", "auth", ..rest] -> auth_routes.handle_auth_route(req, rest, ctx)

    // Protected API routes - require JWT authentication
    ["api", ..rest] -> {
      use claims <- auth.require_jwt_auth(req)
      route_api(req, ctx, rest, claims)
    }

    // All other routes return 404 (frontend handles SPA routing)
    _ -> wisp.not_found()
  }
}

/// Route API requests to appropriate controllers with permission checks
fn route_api(
  req: Request,
  ctx: Context,
  path: List(String),
  claims: JwtClaims,
) -> Response {
  case path {
    // Organizations API (includes nested workspaces, projects, chats, tasks)
    ["organizations", ..rest] -> {
      let permission = auth.permission_for_method("organizations", req.method)
      case jwt.has_permission(claims, permission) {
        True -> organizations_routes.handle_organizations_route(req, rest, ctx)
        False -> auth.forbidden_response("Requires " <> permission)
      }
    }

    // Models API
    ["models"] -> {
      let permission = auth.permission_for_method("models", req.method)
      case jwt.has_permission(claims, permission) {
        True -> models.handle_models(req)
        False -> auth.forbidden_response("Requires " <> permission)
      }
    }
    ["models", author, model_name] -> {
      let permission = auth.permission_for_method("models", req.method)
      case jwt.has_permission(claims, permission) {
        True -> models.handle_model_by_name(req, author <> "/" <> model_name)
        False -> auth.forbidden_response("Requires " <> permission)
      }
    }
    ["models", model_name] -> {
      let permission = auth.permission_for_method("models", req.method)
      case jwt.has_permission(claims, permission) {
        True -> models.handle_model_by_name(req, model_name)
        False -> auth.forbidden_response("Requires " <> permission)
      }
    }

    // Chats API
    ["chats", ..rest] -> {
      let permission = auth.permission_for_method("chats", req.method)
      case jwt.has_permission(claims, permission) {
        True -> chats_routes.handle_chats_route(req, rest, ctx)
        False -> auth.forbidden_response("Requires " <> permission)
      }
    }

    // Projects API
    ["projects", ..rest] -> {
      let permission = auth.permission_for_method("projects", req.method)
      case jwt.has_permission(claims, permission) {
        True -> projects_routes.handle_projects_route(req, rest, ctx)
        False -> auth.forbidden_response("Requires " <> permission)
      }
    }

    // Tasks API
    ["tasks", ..rest] -> {
      let permission = auth.permission_for_method("tasks", req.method)
      case jwt.has_permission(claims, permission) {
        True -> tasks_routes.handle_tasks_route(req, rest, ctx)
        False -> auth.forbidden_response("Requires " <> permission)
      }
    }

    // Sources API
    ["sources", ..rest] -> {
      let permission = auth.permission_for_method("sources", req.method)
      case jwt.has_permission(claims, permission) {
        True -> sources_routes.handle_sources_route(req, rest, ctx)
        False -> auth.forbidden_response("Requires " <> permission)
      }
    }

    _ -> wisp.not_found()
  }
}
