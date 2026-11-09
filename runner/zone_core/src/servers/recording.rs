//! A stand-in MCP server that writes down the environment it was started
//! with, for pinning how the servers Zone launches are routed. Compiled only
//! for tests and the `test-support` feature.

use std::fs;
use std::path::Path;

use serde_json::json;
use zone_routing::BYPASS;

/// The proxy a test routes Zone through.
pub const PROXY: &str = "http://127.0.0.1:28888";

/// The proxy the stand-in's own configuration names, which Zone's must
/// override.
pub const WRONG_PROXY: &str = "http://wrong:8888";

/// The name [`document`] configures its stand-in server under.
pub const RECORDER: &str = "recorder";

const PROXY_VARIABLES: [&str; 6] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];

const BYPASS_VARIABLES: [&str; 2] = ["NO_PROXY", "no_proxy"];

const EVERY_HOST: &str = "*";

/// A `ZONE_MCP_SERVERS` document naming one server that writes its
/// environment to `path` and exits without answering, its own `env` routing
/// it through [`WRONG_PROXY`] and bypassing every host.
pub fn document(path: &Path) -> String {
    json!({"mcpServers": {RECORDER: {
        "command": "sh",
        "args": ["-c", format!("env > '{}'", path.display())],
        "env": {
            "HTTPS_PROXY": WRONG_PROXY,
            "http_proxy": WRONG_PROXY,
            "NO_PROXY": EVERY_HOST,
            "no_proxy": EVERY_HOST
        }
    }}})
    .to_string()
}

/// Panics unless the environment written to `path` routes every client
/// through [`PROXY`] and bypasses only Zone's own [`BYPASS`].
pub fn assert_routed(path: &Path) {
    let environment = fs::read_to_string(path).expect("the server recorded its environment");
    let lines = environment.lines().collect::<Vec<_>>();
    for name in PROXY_VARIABLES {
        let expected = format!("{name}={PROXY}");
        assert!(lines.contains(&expected.as_str()), "{name}: {environment}");
    }
    let bypass = BYPASS.join(",");
    for name in BYPASS_VARIABLES {
        let expected = format!("{name}={bypass}");
        assert!(lines.contains(&expected.as_str()), "{name}: {environment}");
    }
}
