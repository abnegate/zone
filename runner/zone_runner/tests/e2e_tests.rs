//! End-to-end tests for the zone-runner daemon.
//!
//! These tests spawn the actual zone-runner binary and communicate with it
//! via the NDJSON protocol over stdin/stdout.

use base64::prelude::*;
use serde::Serialize;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Helper struct for managing a running daemon process
struct DaemonProcess {
    child: Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl DaemonProcess {
    /// Spawn a new daemon process
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zone-runner"))
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

    /// Close stdin and wait for the process to exit
    fn shutdown(mut self) -> std::process::ExitStatus {
        drop(self.stdin);
        self.child.wait().expect("Failed to wait for child")
    }
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
#[allow(dead_code)]
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

// ============================================================================
// Basic Protocol Tests
// ============================================================================

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

// ============================================================================
// Command Execution Tests
// ============================================================================

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

// ============================================================================
// Error Handling Tests
// ============================================================================

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

    daemon.shutdown();
}

// ============================================================================
// Sequence Number Tests
// ============================================================================

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

// ============================================================================
// Graceful Shutdown Tests
// ============================================================================

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

// ============================================================================
// Duration Tracking Tests
// ============================================================================

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

// ============================================================================
// PID Tracking Tests
// ============================================================================

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
