//! Zone's streamable HTTP stub server, `tests/fixtures/stub_mcp_http_server.py`, run as a child
//! of the test that attaches it.

use std::path::PathBuf;
use std::process::Stdio;

use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;

const INTERPRETER: &str = "python3";
const ISOLATED: &str = "-I";
const SCRIPT: [&str; 3] = ["tests", "fixtures", "stub_mcp_http_server.py"];
const READY: &str = "PORT=";
const HOST: &str = "127.0.0.1";
const PATH: &str = "/mcp";

/// The stub, listening for as long as this is held. It offers one tool, `delegate`.
pub struct Stub {
    port: u16,
    _process: Child,
}

impl Stub {
    /// Starts the stub with `arguments` (`--token <token>` makes it demand that bearer token,
    /// `--record <path>` appends every request's headers to that file) and returns once it has printed the port it listens on, which it does after binding.
    pub async fn start(arguments: &[&str]) -> Self {
        let script: PathBuf = [env!("CARGO_MANIFEST_DIR")]
            .into_iter()
            .chain(SCRIPT)
            .collect();
        let mut process = Command::new(INTERPRETER)
            .arg(ISOLATED)
            .arg(script)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("python3 starts the stub server");
        let output = process.stdout.take().expect("the stub's output is piped");
        let mut ready = String::new();
        BufReader::new(output)
            .read_line(&mut ready)
            .await
            .expect("the stub's first line is read");
        let port = ready
            .trim()
            .strip_prefix(READY)
            .and_then(|port| port.parse().ok())
            .unwrap_or_else(|| panic!("the stub printed {ready:?} instead of its port"));
        Self {
            port,
            _process: process,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn url(&self) -> String {
        format!("http://{HOST}:{}{PATH}", self.port)
    }
}
