//! Real `zone-server` processes for tests about running several replicas.
//!
//! Each replica is the built binary, started with a cleared environment and
//! only the variables [`Environment`] gives it, on a port of its own. Every
//! replica of one test shares the test's database, Valkey, LiteLLM, agent
//! state directory and encryption key, as the pods of one deployment do.

mod account;
mod environment;

use std::fs::File;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use reqwest::StatusCode;
use serde_json::Value;
use tempfile::TempDir;
use tokio::process::{Child, Command};
use uuid::Uuid;
use zone_server::services::fleet::identity::NAME;

pub use account::Account;
pub use environment::Environment;

const BINARY: &str = env!("CARGO_BIN_EXE_zone-server");
const HEALTH: &str = "/health";
const LOG: &str = "server.log";
const STARTUP: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(100);
const ATTEMPTS: usize = 3;
const BIND_FAILURE: &str = "Failed to bind";
const LOG_TAIL_LINES: usize = 200;

pub struct Replica {
    child: Child,
    port: u16,
    name: String,
    home: TempDir,
    http: reqwest::Client,
}

impl Replica {
    /// Starts a replica and waits until it answers `/health`. A port another
    /// test took between choosing and binding it is retried on a new one.
    pub async fn start(environment: &Environment) -> Self {
        let mut failure = String::new();
        for _ in 0..ATTEMPTS {
            let home = TempDir::new().expect("a home for the replica");
            let port = free_port();
            let log = File::create(home.path().join(LOG)).expect("the replica's log");
            let variables = environment.variables(port, home.path());
            let name = variables
                .iter()
                .find(|(variable, _)| variable == NAME)
                .map(|(_, value)| value.clone())
                .expect("the replica has a runner name");
            let child = Command::new(BINARY)
                .env_clear()
                .envs(variables)
                .current_dir(home.path())
                .stdin(Stdio::null())
                .stdout(log.try_clone().expect("the replica's log"))
                .stderr(log)
                .kill_on_drop(true)
                .spawn()
                .expect("the server binary starts");
            let mut replica = Self {
                child,
                port,
                name,
                home,
                http: reqwest::Client::new(),
            };
            match replica.healthy().await {
                Ok(()) => return replica,
                Err(log) if log.contains(BIND_FAILURE) => failure = log,
                Err(log) => panic!("the replica never became healthy:\n{}", tail(&log)),
            }
        }
        panic!("no port the replica could bind:\n{}", tail(&failure));
    }

    async fn healthy(&mut self) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + STARTUP;
        while tokio::time::Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return Err(self.log());
            }
            if let Ok(response) = self.http.get(self.url(HEALTH)).send().await
                && response.status() == StatusCode::OK
            {
                return Ok(());
            }
            tokio::time::sleep(POLL).await;
        }
        Err(self.log())
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The runner name the process registers its fleet runner under.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn websocket(&self, path: &str) -> String {
        format!("ws://127.0.0.1:{}{path}", self.port)
    }

    pub fn chat_socket(&self, chat: Uuid) -> String {
        self.websocket(&format!("/ws/chats/{chat}"))
    }

    /// The JSON body of a POST, whatever its status.
    pub async fn post(&self, path: &str, body: &Value, token: Option<&str>) -> Value {
        self.post_status(path, body, token).await.1
    }

    /// The status and JSON body of a POST.
    pub async fn post_status(
        &self,
        path: &str,
        body: &Value,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut request = self.http.post(self.url(path)).json(body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.expect("the replica answers");
        let status = response.status();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    pub async fn get(&self, path: &str, token: Option<&str>) -> (StatusCode, Value) {
        let mut request = self.http.get(self.url(path));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.expect("the replica answers");
        let status = response.status();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    /// Stops the process with SIGKILL, as a node failure would, and reaps it.
    pub async fn kill(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }

    /// Asks the process to stop with SIGTERM, as Kubernetes does.
    pub fn terminate(&self) {
        let pid = self.child.id().expect("the replica is running");
        let status = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .expect("kill runs");
        assert!(status.success(), "SIGTERM reached the replica");
    }

    /// How the process exited, if it does within `within`.
    pub async fn exited(&mut self, within: Duration) -> Option<ExitStatus> {
        tokio::time::timeout(within, self.child.wait())
            .await
            .ok()?
            .ok()
    }

    /// Everything the process logged so far.
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.home.path().join(LOG)).unwrap_or_default()
    }
}

impl Drop for Replica {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        if std::thread::panicking() {
            eprintln!(
                "--- replica on port {} ---\n{}",
                self.port,
                tail(&self.log())
            );
        }
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port()
}

fn tail(log: &str) -> String {
    let lines: Vec<&str> = log.lines().collect();
    lines[lines.len().saturating_sub(LOG_TAIL_LINES)..].join("\n")
}
