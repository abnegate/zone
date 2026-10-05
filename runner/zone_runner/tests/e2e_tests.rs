//! End-to-end tests for the zone-runner daemon.
//!
//! These tests spawn the actual zone-runner binary and communicate with it
//! via the NDJSON protocol over stdin/stdout.

use abnegate_exec::{PROXY_BYPASS_VARIABLE, PROXY_URL_VARIABLE as EXECUTOR_PROXY_URL_VARIABLE};
use base64::prelude::*;
use serde::Serialize;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use zone_routing::{BYPASS, PROXY_URL_VARIABLE};

const ROUTED_PROXY: &str = "http://127.0.0.1:28888";

fn runner(variables: &[(&str, &str)]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zone-runner"));
    for name in [
        PROXY_URL_VARIABLE,
        EXECUTOR_PROXY_URL_VARIABLE,
        PROXY_BYPASS_VARIABLE,
    ] {
        command.env_remove(name);
    }
    command.envs(variables.iter().copied());
    command
}

fn stdout_of(messages: &[Value]) -> String {
    messages
        .iter()
        .filter(|message| message["type"] == "RunStdout")
        .map(|message| {
            let data = message["data"].as_str().expect("RunStdout carries data");
            let bytes = BASE64_STANDARD
                .decode(data)
                .expect("RunStdout data is base64");
            String::from_utf8(bytes).expect("the job writes UTF-8")
        })
        .collect()
}

fn assert_routed(environment: &str) {
    let lines: Vec<&str> = environment.lines().collect();
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
        assert!(
            lines.contains(&format!("{name}={ROUTED_PROXY}").as_str()),
            "{name}: {environment}"
        );
    }
    assert!(
        lines.contains(&format!("NO_PROXY={}", BYPASS.join(",")).as_str()),
        "{environment}"
    );
}

/// Helper struct for managing a running daemon process
struct DaemonProcess {
    child: Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl DaemonProcess {
    /// Spawn a new daemon process
    fn spawn() -> Self {
        Self::spawn_with(&[])
    }

    fn spawn_with(variables: &[(&str, &str)]) -> Self {
        let mut child = runner(variables)
            .args(["serve", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Failed to spawn zone-runner");

        let stdin = child.stdin.take().expect("Failed to get stdin");
        let stdout = child.stdout.take().expect("Failed to get stdout");
        let reader = BufReader::new(stdout);

        Self {
            child,
            stdin,
            reader,
        }
    }

    fn handshake(&mut self) {
        self.send(&Hello {
            msg_type: "Hello".to_string(),
            protocol_version: "1.0".to_string(),
            capabilities: Some(vec!["stdin".to_string()]),
        });
        let acknowledgement = self.recv();
        assert_eq!(acknowledgement["type"], "HelloAck", "{acknowledgement}");
    }

    /// Send a message to the daemon
    fn send<T: Serialize>(&mut self, msg: &T) {
        let json = serde_json::to_string(msg).expect("Failed to serialize message");
        writeln!(self.stdin, "{}", json).expect("Failed to write to stdin");
        self.stdin.flush().expect("Failed to flush stdin");
    }

    /// Read a message from the daemon
    fn recv(&mut self) -> Value {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .expect("Failed to read line");
        serde_json::from_str(&line).expect("Failed to parse JSON")
    }

    /// Read messages until a predicate is satisfied or timeout
    fn recv_until<F>(&mut self, timeout: Duration, mut predicate: F) -> Vec<Value>
    where
        F: FnMut(&Value) -> bool,
    {
        let start = std::time::Instant::now();
        let mut messages = Vec::new();

        while start.elapsed() < timeout {
            // Set a short read timeout using the file descriptor
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                        let done = predicate(&msg);
                        messages.push(msg);
                        if done {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }

        messages
    }

    /// Kill the daemon once `limit` has passed, so a daemon that stops
    /// answering fails the test instead of hanging it. Dropping the
    /// watchdog disarms it.
    fn start(&mut self, job_id: &str, command: &str, args: &[&str]) {
        self.send(&RunStart {
            msg_type: "RunStart".to_string(),
            job_id: job_id.to_string(),
            workspace: "/tmp".to_string(),
            command: command.to_string(),
            args: args.iter().map(|argument| argument.to_string()).collect(),
            timeout_ms: Some(60000),
        });
        let started = self.recv_until(Duration::from_secs(5), |message| {
            message["type"] == "RunStarted" || message["type"] == "RunError"
        });
        assert_eq!(
            started.last().map(|message| &message["type"]),
            Some(&Value::from("RunStarted")),
            "{started:?}"
        );
    }

    fn write_stdin(&mut self, job_id: &str, data: String, eof: bool) {
        self.send(&RunStdin {
            msg_type: "RunStdin".to_string(),
            job_id: job_id.to_string(),
            data,
            eof,
        });
    }

    fn finish(&mut self) -> Vec<Value> {
        self.recv_until(Duration::from_secs(10), |message| {
            message["type"] == "RunExit"
                || (message["type"] == "RunError" && message["job_id"] != "")
        })
    }

    fn watchdog(&self, limit: Duration) -> Watchdog {
        let pid = self.child.id().to_string();
        let (armed, disarmed) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            if disarmed.recv_timeout(limit) == Err(mpsc::RecvTimeoutError::Timeout) {
                let _ = Command::new("kill").args(["-KILL", &pid]).status();
            }
        });
        Watchdog { _armed: armed }
    }

    /// Close stdin and wait for the process to exit
    fn shutdown(mut self) -> std::process::ExitStatus {
        drop(self.stdin);
        self.child.wait().expect("Failed to wait for child")
    }
}

struct Watchdog {
    _armed: mpsc::Sender<()>,
}

// Protocol message types for testing
#[derive(Serialize)]
struct Hello {
    #[serde(rename = "type")]
    msg_type: String,
    protocol_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<Vec<String>>,
}

#[derive(Serialize)]
struct RunStart {
    #[serde(rename = "type")]
    msg_type: String,
    job_id: String,
    workspace: String,
    command: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_ms: Option<u64>,
}

#[derive(Serialize)]
struct RunStdin {
    #[serde(rename = "type")]
    msg_type: String,
    job_id: String,
    data: String,
    eof: bool,
}

#[derive(Serialize)]
struct RunCancel {
    #[serde(rename = "type")]
    msg_type: String,
    job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    force: Option<bool>,
}

#[derive(Serialize)]
struct Ping {
    #[serde(rename = "type")]
    msg_type: String,
    id: String,
}

#[test]
fn test_handshake() {
    let mut daemon = DaemonProcess::spawn();

    // Send Hello
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);

    // Receive HelloAck
    let response = daemon.recv();
    assert_eq!(response["type"], "HelloAck");
    assert!(
        response["protocol_version"]
            .as_str()
            .unwrap()
            .starts_with("1.")
    );
    assert!(response["runner_version"].is_string());
    assert!(response["capabilities"].is_array());

    daemon.shutdown();
}

#[test]
fn test_handshake_with_capabilities() {
    let mut daemon = DaemonProcess::spawn();

    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: Some(vec!["streaming".to_string()]),
    };
    daemon.send(&hello);

    let response = daemon.recv();
    assert_eq!(response["type"], "HelloAck");

    daemon.shutdown();
}

#[test]
fn test_ping_pong() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake first
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv(); // HelloAck

    // Send Ping
    let ping = Ping {
        msg_type: "Ping".to_string(),
        id: "42".to_string(),
    };
    daemon.send(&ping);

    // Receive Pong
    let response = daemon.recv();
    assert_eq!(response["type"], "Pong");
    assert_eq!(response["id"], "42");

    daemon.shutdown();
}

#[test]
fn test_multiple_pings() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    // Send multiple pings
    for i in 1..=5 {
        let ping = Ping {
            msg_type: "Ping".to_string(),
            id: format!("{}", i),
        };
        daemon.send(&ping);

        let response = daemon.recv();
        assert_eq!(response["type"], "Pong");
        assert_eq!(response["id"], format!("{}", i));
    }

    daemon.shutdown();
}

#[test]
fn test_simple_command() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    // Run a simple echo command
    let run = RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-1".to_string(),
        workspace: "/tmp".to_string(),
        command: "echo".to_string(),
        args: vec!["hello world".to_string()],
        timeout_ms: Some(5000),
    };
    daemon.send(&run);

    // Collect messages until we get RunExit
    let messages = daemon.recv_until(Duration::from_secs(5), |msg| {
        msg["type"] == "RunExit" || msg["type"] == "RunError"
    });

    // Verify we got the expected messages
    let types: Vec<&str> = messages
        .iter()
        .map(|m| m["type"].as_str().unwrap())
        .collect();

    assert!(
        types.contains(&"RunStarted"),
        "Missing RunStarted: {:?}",
        types
    );
    assert!(
        types.contains(&"RunExit") || types.contains(&"RunError"),
        "Missing terminal message: {:?}",
        types
    );

    // Check stdout content
    let stdout_msgs: Vec<&Value> = messages
        .iter()
        .filter(|m| m["type"] == "RunStdout")
        .collect();
    assert!(!stdout_msgs.is_empty(), "No stdout messages received");

    // Decode and verify output
    let data = stdout_msgs[0]["data"].as_str().unwrap();
    let decoded = BASE64_STANDARD.decode(data).unwrap();
    let text = String::from_utf8(decoded).unwrap();
    assert!(
        text.contains("hello world"),
        "Output doesn't contain expected text: {}",
        text
    );

    daemon.shutdown();
}

#[test]
fn test_command_with_exit_code() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    // Run command that exits with code 42
    let run = RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-exit".to_string(),
        workspace: "/tmp".to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), "exit 42".to_string()],
        timeout_ms: Some(5000),
    };
    daemon.send(&run);

    let messages = daemon.recv_until(Duration::from_secs(5), |msg| {
        msg["type"] == "RunExit" || msg["type"] == "RunError"
    });

    let exit_msg = messages.iter().find(|m| m["type"] == "RunExit");
    assert!(exit_msg.is_some(), "No RunExit message");
    assert_eq!(exit_msg.unwrap()["exit_code"], 42);

    daemon.shutdown();
}

#[test]
fn test_command_stderr() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    // Run command that writes to stderr
    let run = RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-stderr".to_string(),
        workspace: "/tmp".to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), "echo error message >&2".to_string()],
        timeout_ms: Some(5000),
    };
    daemon.send(&run);

    let messages = daemon.recv_until(Duration::from_secs(5), |msg| {
        msg["type"] == "RunExit" || msg["type"] == "RunError"
    });

    // Check stderr content
    let stderr_msgs: Vec<&Value> = messages
        .iter()
        .filter(|m| m["type"] == "RunStderr")
        .collect();
    assert!(!stderr_msgs.is_empty(), "No stderr messages received");

    let data = stderr_msgs[0]["data"].as_str().unwrap();
    let decoded = BASE64_STANDARD.decode(data).unwrap();
    let text = String::from_utf8(decoded).unwrap();
    assert!(
        text.contains("error message"),
        "Stderr doesn't contain expected text: {}",
        text
    );

    daemon.shutdown();
}

#[test]
fn test_multiple_jobs() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    // Start multiple jobs
    for i in 1..=3 {
        let run = RunStart {
            msg_type: "RunStart".to_string(),
            job_id: format!("job-{}", i),
            workspace: "/tmp".to_string(),
            command: "echo".to_string(),
            args: vec![format!("output-{}", i)],
            timeout_ms: Some(5000),
        };
        daemon.send(&run);
    }

    // Collect messages - we expect 3 RunStarted + 3 RunStdout + 3 RunExit = 9+ messages
    // But just wait for a reasonable time
    let mut exit_count = 0;
    let messages = daemon.recv_until(Duration::from_secs(5), |msg| {
        if msg["type"] == "RunExit" {
            exit_count += 1;
        }
        exit_count >= 3
    });

    // Verify we got messages for all jobs
    let job_ids: std::collections::HashSet<&str> = messages
        .iter()
        .filter_map(|m| m["job_id"].as_str())
        .collect();

    assert!(job_ids.contains("job-1"), "Missing job-1 messages");
    assert!(job_ids.contains("job-2"), "Missing job-2 messages");
    assert!(job_ids.contains("job-3"), "Missing job-3 messages");

    daemon.shutdown();
}

#[test]
fn test_invalid_workspace() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    // Run with invalid workspace
    let run = RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-invalid".to_string(),
        workspace: "/nonexistent/path/that/does/not/exist".to_string(),
        command: "echo".to_string(),
        args: vec!["hello".to_string()],
        timeout_ms: Some(5000),
    };
    daemon.send(&run);

    let messages = daemon.recv_until(Duration::from_secs(5), |msg| msg["type"] == "RunError");

    let error_msg = messages.iter().find(|m| m["type"] == "RunError");
    assert!(error_msg.is_some(), "Expected RunError message");
    assert_eq!(error_msg.unwrap()["error_code"], "invalid_workspace");

    daemon.shutdown();
}

#[test]
fn test_invalid_command() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    // Run with invalid command
    let run = RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-badcmd".to_string(),
        workspace: "/tmp".to_string(),
        command: "/nonexistent/binary/command".to_string(),
        args: vec![],
        timeout_ms: Some(5000),
    };
    daemon.send(&run);

    let messages = daemon.recv_until(Duration::from_secs(5), |msg| msg["type"] == "RunError");

    let error_msg = messages.iter().find(|m| m["type"] == "RunError");
    assert!(error_msg.is_some(), "Expected RunError message");
    assert_eq!(error_msg.unwrap()["error_code"], "spawn_failed");

    daemon.shutdown();
}

#[test]
fn test_message_before_handshake() {
    let mut daemon = DaemonProcess::spawn();

    // Send a command without handshake first
    let run = RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-early".to_string(),
        workspace: "/tmp".to_string(),
        command: "echo".to_string(),
        args: vec!["hello".to_string()],
        timeout_ms: Some(5000),
    };
    daemon.send(&run);

    // Should get an error
    let response = daemon.recv();
    assert_eq!(response["type"], "RunError");

    daemon.shutdown();
}

#[test]
fn test_invalid_protocol_version() {
    let mut daemon = DaemonProcess::spawn();

    // Send Hello with unsupported version
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "99.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);

    // Should get an error
    let response = daemon.recv();
    assert_eq!(response["type"], "RunError");

    let _watchdog = daemon.watchdog(Duration::from_secs(15));
    let _ = writeln!(
        daemon.stdin,
        r#"{{"type":"RunStart","job_id":"job-after-refusal","workspace":"/tmp","command":"echo"}}"#
    );
    let _ = daemon.stdin.flush();
    let waited = Instant::now();
    let after = daemon.recv_until(Duration::from_secs(10), |_| false);

    assert!(
        waited.elapsed() < Duration::from_secs(10),
        "the daemon kept running after refusing the version"
    );
    assert!(
        after.iter().all(|message| message["type"] != "RunStarted"),
        "a job ran without a handshake: {after:?}"
    );
    let status = daemon.shutdown();
    assert!(!status.success(), "{status}");
}

#[test]
fn test_sequence_numbers() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    // Run command with multiple lines of output
    let run = RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-seq".to_string(),
        workspace: "/tmp".to_string(),
        command: "sh".to_string(),
        args: vec![
            "-c".to_string(),
            "echo line1; echo line2; echo line3".to_string(),
        ],
        timeout_ms: Some(5000),
    };
    daemon.send(&run);

    let messages = daemon.recv_until(Duration::from_secs(5), |msg| {
        msg["type"] == "RunExit" || msg["type"] == "RunError"
    });

    // Check that sequence numbers are monotonically increasing
    let stdout_seqs: Vec<u64> = messages
        .iter()
        .filter(|m| m["type"] == "RunStdout")
        .map(|m| m["sequence"].as_u64().unwrap())
        .collect();

    for i in 1..stdout_seqs.len() {
        assert!(
            stdout_seqs[i] > stdout_seqs[i - 1],
            "Sequence numbers should be monotonically increasing: {:?}",
            stdout_seqs
        );
    }

    daemon.shutdown();
}

#[test]
fn test_graceful_shutdown_on_eof() {
    let daemon = DaemonProcess::spawn();

    // Just close stdin immediately
    let status = daemon.shutdown();

    // Should exit cleanly
    assert!(status.success(), "Daemon should exit cleanly on EOF");
}

#[test]
fn test_shutdown_with_running_job() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    // Start a long-running job
    let run = RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-long".to_string(),
        workspace: "/tmp".to_string(),
        command: "sleep".to_string(),
        args: vec!["60".to_string()],
        timeout_ms: Some(60000),
    };
    daemon.send(&run);

    // Wait for RunStarted
    let _ = daemon.recv();

    // Shutdown while job is running
    let status = daemon.shutdown();

    // Should still exit (after cancelling the job)
    assert!(
        status.success(),
        "Daemon should handle shutdown with running job"
    );
}

#[test]
fn test_duration_in_exit() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    // Run a command that takes a measurable amount of time
    let run = RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-duration".to_string(),
        workspace: "/tmp".to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), "sleep 0.1 && echo done".to_string()],
        timeout_ms: Some(5000),
    };
    daemon.send(&run);

    let messages = daemon.recv_until(Duration::from_secs(5), |msg| msg["type"] == "RunExit");

    let exit_msg = messages.iter().find(|m| m["type"] == "RunExit");
    assert!(exit_msg.is_some(), "No RunExit message");

    let duration_ms = exit_msg.unwrap()["duration_ms"].as_u64().unwrap();
    assert!(
        duration_ms >= 100,
        "Duration should be at least 100ms, got {}",
        duration_ms
    );

    daemon.shutdown();
}

#[test]
fn test_pid_in_started() {
    let mut daemon = DaemonProcess::spawn();

    // Handshake
    let hello = Hello {
        msg_type: "Hello".to_string(),
        protocol_version: "1.0".to_string(),
        capabilities: None,
    };
    daemon.send(&hello);
    daemon.recv();

    let run = RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-pid".to_string(),
        workspace: "/tmp".to_string(),
        command: "echo".to_string(),
        args: vec!["test".to_string()],
        timeout_ms: Some(5000),
    };
    daemon.send(&run);

    let messages = daemon.recv_until(Duration::from_secs(5), |msg| msg["type"] == "RunStarted");

    let started_msg = messages.iter().find(|m| m["type"] == "RunStarted");
    assert!(started_msg.is_some(), "No RunStarted message");

    let pid = started_msg.unwrap()["pid"].as_u64();
    assert!(pid.is_some(), "RunStarted should have pid");
    assert!(pid.unwrap() > 0, "PID should be positive");

    daemon.shutdown();
}

#[test]
fn stdin_sent_to_a_job_reaches_it() {
    let mut daemon = DaemonProcess::spawn();
    daemon.handshake();
    daemon.start("job-stdin", "cat", &[]);

    daemon.write_stdin("job-stdin", BASE64_STANDARD.encode("piped through\n"), true);

    assert_piped_through(&daemon.finish());
    daemon.shutdown();
}

fn assert_piped_through(messages: &[Value]) {
    assert_eq!(stdout_of(messages), "piped through\n", "{messages:?}");
    let exit = messages
        .iter()
        .find(|message| message["type"] == "RunExit")
        .unwrap_or_else(|| panic!("cat never exited: {messages:?}"));
    assert_eq!(exit["exit_code"], 0, "{exit}");
}

#[test]
fn invalid_stdin_leaves_the_job_running() {
    let mut daemon = DaemonProcess::spawn();
    let _watchdog = daemon.watchdog(Duration::from_secs(60));
    daemon.handshake();
    daemon.start("job-invalid-stdin", "cat", &[]);

    daemon.write_stdin("job-invalid-stdin", "not base64!".to_string(), false);
    let refused = daemon.recv();
    daemon.write_stdin(
        "job-invalid-stdin",
        BASE64_STANDARD.encode("piped through\n"),
        true,
    );

    assert_eq!(refused["type"], "RunError", "{refused}");
    assert_eq!(refused["job_id"], "", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("job-invalid-stdin")),
        "{refused}"
    );
    assert_piped_through(&daemon.finish());
    daemon.shutdown();
}

#[test]
fn a_duplicate_run_start_leaves_the_running_job_alone() {
    let mut daemon = DaemonProcess::spawn();
    let _watchdog = daemon.watchdog(Duration::from_secs(60));
    daemon.handshake();
    daemon.start("job-duplicate", "cat", &[]);

    daemon.send(&RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-duplicate".to_string(),
        workspace: "/tmp".to_string(),
        command: "cat".to_string(),
        args: Vec::new(),
        timeout_ms: Some(60000),
    });
    let refused = daemon.recv();
    daemon.write_stdin(
        "job-duplicate",
        BASE64_STANDARD.encode("piped through\n"),
        true,
    );

    assert_eq!(refused["type"], "RunError", "{refused}");
    assert_eq!(refused["job_id"], "", "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("job-duplicate")),
        "{refused}"
    );
    assert_piped_through(&daemon.finish());
    daemon.shutdown();
}

#[test]
fn a_job_is_routed_through_the_tool_runner_proxy() {
    let mut daemon = DaemonProcess::spawn_with(&[(PROXY_URL_VARIABLE, ROUTED_PROXY)]);
    daemon.handshake();
    daemon.send(&RunStart {
        msg_type: "RunStart".to_string(),
        job_id: "job-routed".to_string(),
        workspace: "/tmp".to_string(),
        command: "env".to_string(),
        args: Vec::new(),
        timeout_ms: Some(5000),
    });

    let messages = daemon.recv_until(Duration::from_secs(5), |message| {
        message["type"] == "RunExit" || message["type"] == "RunError"
    });

    assert_routed(&stdout_of(&messages));

    daemon.shutdown();
}

#[test]
fn a_one_shot_command_is_routed_through_the_tool_runner_proxy() {
    let output = runner(&[(PROXY_URL_VARIABLE, ROUTED_PROXY)])
        .args(["exec", "--workspace", "/tmp", "--cmd", "env"])
        .stdin(Stdio::null())
        .output()
        .expect("zone-runner exec runs");

    assert!(output.status.success(), "{output:?}");
    assert_routed(&String::from_utf8_lossy(&output.stdout));
}

#[test]
fn a_job_that_never_reads_its_stdin_cannot_stall_the_daemon() {
    let mut daemon = DaemonProcess::spawn();
    let _watchdog = daemon.watchdog(Duration::from_secs(60));
    daemon.handshake();
    daemon.start("job-sleeping", "sleep", &["30"]);

    let chunk = BASE64_STANDARD.encode(vec![b'x'; 64 * 1024]);
    for _ in 0..200 {
        daemon.write_stdin("job-sleeping", chunk.clone(), false);
    }
    let pinged = Instant::now();
    daemon.send(&Ping {
        msg_type: "Ping".to_string(),
        id: "after-stdin".to_string(),
    });
    let answered = daemon.recv_until(Duration::from_secs(10), |message| message["type"] == "Pong");

    let pong = answered.last().expect("the daemon answered");
    assert_eq!(pong["type"], "Pong", "{pong}");
    assert_eq!(pong["id"], "after-stdin", "{pong}");
    assert!(
        pinged.elapsed() < Duration::from_secs(10),
        "the Pong took {:?}",
        pinged.elapsed()
    );
    assert!(
        answered
            .iter()
            .filter(|message| message["type"] == "RunError")
            .all(|message| message["job_id"] == "" && message["error_code"] == "invalid_message"),
        "a refused write named the job: {answered:?}"
    );

    daemon.send(&RunCancel {
        msg_type: "RunCancel".to_string(),
        job_id: "job-sleeping".to_string(),
        force: Some(true),
    });
    let cancelled = daemon.recv_until(Duration::from_secs(10), |message| {
        message["type"] == "RunError" && message["job_id"] == "job-sleeping"
    });
    let error = cancelled.last().expect("the daemon answered the cancel");
    assert_eq!(error["job_id"], "job-sleeping", "{error}");
    assert_eq!(error["error_code"], "cancelled", "{error}");

    daemon.shutdown();
}

#[test]
fn stdin_for_a_job_that_is_not_running_is_answered() {
    let mut daemon = DaemonProcess::spawn();
    let _watchdog = daemon.watchdog(Duration::from_secs(60));
    daemon.handshake();
    daemon.start("job-finished", "true", &[]);
    daemon.finish();

    for job_id in ["job-unknown", "job-finished"] {
        daemon.write_stdin(job_id, BASE64_STANDARD.encode("lost\n"), false);
        let answer = daemon.recv();

        assert_eq!(answer["type"], "RunError", "{answer}");
        assert_eq!(answer["job_id"], job_id, "{answer}");
        assert_eq!(answer["error_code"], "job_not_found", "{answer}");
    }

    daemon.shutdown();
}

#[test]
fn a_one_shot_command_sees_its_stdin_closed() {
    let mut runner = runner(&[])
        .args(["exec", "--workspace", "/tmp", "--cmd", "cat"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("zone-runner exec starts");

    let started = Instant::now();
    let status = loop {
        if let Some(status) = runner.try_wait().expect("zone-runner exec is waited on") {
            break Some(status);
        }
        if started.elapsed() > Duration::from_secs(10) {
            let _ = runner.kill();
            let _ = runner.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let status = status.expect("cat was still waiting for stdin after 10 seconds");
    assert!(status.success(), "{status}");
}
