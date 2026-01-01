import cache/connection as cache_connection
import database/connection as db_connection
import gleam/erlang/process
import gleam/http/request
import gleam/http/response
import gleam/io
import middleware/metrics
import mist.{type Connection, type ResponseData}
import router
import web.{type Context, Context}
import websocket/pull
import websocket/task_run
import wisp
import wisp/wisp_mist

pub fn main() {
  wisp.configure_logger()

  // Initialize Prometheus metrics
  case metrics.init() {
    Ok(_) -> io.println("Prometheus metrics initialized")
    Error(_) -> io.println("Warning: Failed to initialize metrics")
  }

  // Initialize database connection pool
  case db_connection.connect() {
    Ok(_) -> io.println("Database connection pool started")
    Error(err) -> io.println("Warning: Database not available - " <> err)
  }

  // Initialize cache connection
  let cache = case cache_connection.connect() {
    Ok(client) -> {
      io.println("Cache connection started")
      Ok(client)
    }
    Error(err) -> {
      io.println("Warning: Cache not available - " <> err)
      Error(err)
    }
  }

  // Create application context with shared dependencies
  let ctx = case cache {
    Ok(cache_client) ->
      Context(db: db_connection.named_connection(), cache: cache_client)
    Error(_) -> {
      // If cache unavailable, start without it (will need a fallback)
      io.println("Starting without cache - performance may be degraded")
      // Connect with retry or dummy connection
      case cache_connection.connect() {
        Ok(client) ->
          Context(db: db_connection.named_connection(), cache: client)
        Error(_) -> panic as "Cache connection required"
      }
    }
  }

  let secret = wisp.random_string(64)

  let assert Ok(_) =
    mist.new(fn(req) { handle_mist_request(req, ctx, secret) })
    |> mist.bind("0.0.0.0")
    |> mist.port(8000)
    |> mist.start

  io.println("Server started on http://0.0.0.0:8000")
  process.sleep_forever()
}

/// Handle Mist requests - routes WebSocket vs HTTP
fn handle_mist_request(
  req: request.Request(Connection),
  ctx: Context,
  secret: String,
) -> response.Response(ResponseData) {
  case request.path_segments(req) {
    ["ws", "pull"] -> pull.handle_websocket_upgrade_with_auth(req)
    ["ws", "tasks", run_id] ->
      task_run.handle_websocket_upgrade_with_auth(req, run_id)
    _ -> wisp_mist.handler(router.handle_request(_, ctx), secret)(req)
  }
}
