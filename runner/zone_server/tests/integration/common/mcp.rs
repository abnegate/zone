//! The process environment a test of MCP servers runs in.

use zone_core::variables::Variables;

pub const SERVERS: &str = "ZONE_MCP_SERVERS";

const ENABLED: &str = "ZONE_MCP_ENABLED";
const CONFIGURATION: &str = "ZONE_MCP_CONFIG";
const AUTO_CONNECT: &str = "ZONE_MCP_AUTO_CONNECT";
const AUTO_MAGENTS: &str = "ZONE_MCP_AUTO_MAGENTS";
const NONE: &str = r#"{"mcpServers":{}}"#;
const OFF: &str = "false";

/// The process environment with no instance MCP server, held for the test: not `magents` on the
/// PATH, nor the environment the test binary was started with, nor a `~/.zone/mcp.json`. That
/// file is read whenever neither `ZONE_MCP_SERVERS` nor `ZONE_MCP_CONFIG` names a document, so
/// the first carries a document with no server in it instead of being cleared.
pub fn isolated() -> Variables {
    let mut variables = Variables::isolated(&[ENABLED, CONFIGURATION, AUTO_MAGENTS]);
    variables.set(SERVERS, NONE);
    variables.set(AUTO_CONNECT, OFF);
    variables
}
