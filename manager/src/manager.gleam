import controllers/static as static_controller
import gleam/erlang/process
import gleam/http/request
import gleam/http/response
import mist.{type Connection, type ResponseData}
import router
import websocket/pull
import wisp
import wisp/wisp_mist

pub fn main() {
  wisp.configure_logger()

  let secret = wisp.random_string(64)

  let assert Ok(_) =
    mist.new(fn(req) { handle_mist_request(req, secret) })
    |> mist.bind("0.0.0.0")
    |> mist.port(8000)
    |> mist.start

  process.sleep_forever()
}

/// Handle Mist requests - routes WebSocket vs HTTP
fn handle_mist_request(
  req: request.Request(Connection),
  secret: String,
) -> response.Response(ResponseData) {
  case request.path_segments(req) {
    ["ws", "pull"] -> pull.handle_websocket_upgrade_with_auth(req)
    _ -> wisp_mist.handler(router.handle_request, secret)(req)
  }
}

// =============================================================================
// Re-exports for backwards compatibility with tests
// =============================================================================

/// Parse query string parameters into key-value pairs
/// @internal
pub fn parse_query_params(query: String) -> List(#(String, String)) {
  pull.parse_query_params(query)
}

/// Parse WebSocket pull request JSON
/// @internal
pub fn parse_ws_pull_request(text: String) -> Result(String, String) {
  pull.parse_ws_pull_request(text)
}

/// Validate path segments to prevent directory traversal attacks
/// @internal
pub fn validate_path_segments(segments: List(String)) -> Bool {
  static_controller.validate_path_segments(segments)
}

/// Get content type based on file extension
/// @internal
pub fn get_content_type(path: String) -> String {
  static_controller.get_content_type(path)
}
