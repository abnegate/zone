//! Integration tests for OS-level confinement.
//!
//! These tests exercise:
//! - Seatbelt profile generation, including path escaping
//! - Bubblewrap argument generation
//! - The unsupported-platform path
//! - Fail-closed spawning when confinement cannot be established
//! - Real confined execution on hosts that can prove their sandbox

use base64::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use tool_runner::error::ExecutorError;
use tool_runner::executor::{Backend, CommandExecutor, Confinement, ConfinementError};
use tool_runner::protocol::{
    Capability, ConfinementRequest, ErrorCode, InboundMessage, OutboundMessage,
};

const SECRET: &str = "secret\n";
const GRANTED: &str = "granted\n";

struct Workspace {
    _base: TempDir,
    root: PathBuf,
    granted: PathBuf,
    denied: PathBuf,
}

/// A readable/writable root holding `granted`, next to a `denied` file that no
/// confined command is allowed to see.
fn workspace() -> Workspace {
    let base = TempDir::new().unwrap();
    let root = fs::canonicalize(base.path()).unwrap().join("root");
    fs::create_dir(&root).unwrap();
    let granted = root.join("granted");
    fs::write(&granted, GRANTED).unwrap();
    let denied = fs::canonicalize(base.path()).unwrap().join("denied");
    fs::write(&denied, SECRET).unwrap();

    Workspace {
        _base: base,
        root,
        granted,
        denied,
    }
}

fn request(root: &Path) -> ConfinementRequest {
    ConfinementRequest {
        read_roots: vec![root.to_path_buf()],
        write_roots: vec![root.to_path_buf()],
    }
}

fn confinement(root: &Path, arguments: Vec<String>) -> Confinement {
    Confinement::new("/bin/cat", arguments, root).with_roots(&request(root))
}

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

fn confined_run(job_id: &str, root: &Path, target: &Path) -> InboundMessage {
    InboundMessage::RunStart {
        job_id: job_id.to_string(),
        workspace: root.to_path_buf(),
        command: "/bin/cat".to_string(),
        args: vec![text(target)],
        env: HashMap::new(),
        timeout_ms: Some(15000),
        max_output_bytes: None,
        working_dir: None,
        confinement: Some(request(root)),
    }
}

async fn collect_messages(
    rx: &mut mpsc::Receiver<OutboundMessage>,
    timeout: Duration,
) -> Vec<OutboundMessage> {
    let mut messages = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(message)) => {
                let finished = matches!(
                    message,
                    OutboundMessage::RunExit { .. } | OutboundMessage::RunError { .. }
                );
                messages.push(message);
                if finished {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    messages
}

fn stdout(messages: &[OutboundMessage]) -> String {
    messages
        .iter()
        .filter_map(|message| match message {
            OutboundMessage::RunStdout { data, .. } => Some(data),
            _ => None,
        })
        .map(|data| String::from_utf8(BASE64_STANDARD.decode(data).unwrap()).unwrap())
        .collect()
}

fn exit_code(messages: &[OutboundMessage]) -> Option<i32> {
    messages.iter().find_map(|message| match message {
        OutboundMessage::RunExit { exit_code, .. } => *exit_code,
        _ => None,
    })
}

fn seatbelt_profile(confinement: &Confinement) -> String {
    let invocation = confinement.invocation(Some(Backend::Seatbelt)).unwrap();
    assert_eq!(invocation.program, PathBuf::from("/usr/bin/sandbox-exec"));
    assert_eq!(invocation.arguments[0], "-p");
    assert_eq!(invocation.arguments[2], "--");
    invocation.arguments[1].clone()
}

#[test]
fn test_seatbelt_profile_starts_from_a_full_deny() {
    let workspace = workspace();
    let profile = seatbelt_profile(&confinement(&workspace.root, vec![]));
    let lines: Vec<&str> = profile.lines().collect();

    assert_eq!(lines[0], "(version 1)");
    assert_eq!(lines[1], "(deny default)");
    assert_eq!(lines[2], "(import \"system.sb\")");
}

#[test]
fn test_seatbelt_profile_denies_network_signals_and_host_files() {
    let workspace = workspace();
    let profile = seatbelt_profile(&confinement(&workspace.root, vec![]));

    assert!(profile.contains("(deny network*)"), "{profile}");
    assert!(profile.contains("(deny signal)"), "{profile}");
    assert!(profile.contains("(allow sysctl-read)"), "{profile}");
    assert!(
        profile.contains(r#"(deny file-read* (literal "/private/etc/hosts"))"#),
        "{profile}"
    );
    assert!(
        profile.contains(r#"(deny file-read* (literal "/private/etc/passwd"))"#),
        "{profile}"
    );
}

#[test]
fn test_seatbelt_profile_grants_the_requested_roots() {
    let workspace = workspace();
    let profile = seatbelt_profile(&confinement(&workspace.root, vec![]));
    let root = text(&workspace.root);

    assert!(
        profile.contains(&format!("(allow file-read* (subpath \"{root}\"))")),
        "{profile}"
    );
    assert!(
        profile.contains(&format!("(allow file-write* (subpath \"{root}\"))")),
        "{profile}"
    );
    for tree in ["/System", "/dev", "/usr/lib", "/usr/share"] {
        assert!(
            profile.contains(&format!("(allow file-read* (subpath \"{tree}\"))")),
            "{profile}"
        );
    }
}

#[test]
fn test_seatbelt_profile_pins_the_executable_and_its_ancestors() {
    let workspace = workspace();
    let profile = seatbelt_profile(&confinement(&workspace.root, vec![]));
    let command = fs::canonicalize("/bin/cat").unwrap();
    let command = text(&command);

    assert!(
        profile.contains(&format!("(allow process-exec (literal \"{command}\"))")),
        "{profile}"
    );
    assert!(
        profile.contains(&format!("(allow file-read* (literal \"{command}\"))")),
        "{profile}"
    );
    for ancestor in Path::new(&command)
        .ancestors()
        .filter(|ancestor| ancestor.parent().is_some())
    {
        assert!(
            profile.contains(&format!(
                "(allow file-read-metadata (literal \"{}\"))",
                text(ancestor)
            )),
            "missing metadata for {}\n{profile}",
            ancestor.display()
        );
    }
}

#[test]
fn test_seatbelt_profile_escapes_quotes_backslashes_and_spaces() {
    let base = TempDir::new().unwrap();
    let root = fs::canonicalize(base.path())
        .unwrap()
        .join(r#"odd " name \ with spaces"#);
    fs::create_dir(&root).unwrap();

    let profile = seatbelt_profile(&confinement(&root, vec![]));
    let escaped = text(&root).replace('\\', r"\\").replace('"', "\\\"");

    assert!(
        profile.contains(&format!("(allow file-read* (subpath \"{escaped}\"))")),
        "{profile}"
    );
    assert!(
        profile.contains(&format!("(allow file-write* (subpath \"{escaped}\"))")),
        "{profile}"
    );
    assert!(
        !profile.contains(&format!("subpath \"{}\"", text(&root))),
        "the raw path escaped its string literal\n{profile}"
    );

    for line in profile.lines() {
        let quotes = line
            .char_indices()
            .filter(|(index, character)| *character == '"' && !line[..*index].ends_with('\\'))
            .count();
        assert!(quotes % 2 == 0, "unbalanced string literal in: {line}");
    }
}

#[test]
fn test_seatbelt_profile_rejects_a_newline_root_instead_of_injecting_a_clause() {
    let base = TempDir::new().unwrap();
    let base = fs::canonicalize(base.path()).unwrap();
    let root = base.join("in\n(allow default)\njected");
    fs::create_dir(&root).unwrap();

    let confinement = Confinement::new("/bin/cat", vec![], &base).with_roots(&ConfinementRequest {
        read_roots: vec![root],
        write_roots: vec![],
    });

    assert!(matches!(
        confinement.invocation(Some(Backend::Seatbelt)),
        Err(ConfinementError::ControlCharacterInPath(_))
    ));
}

#[test]
fn test_confinement_rejects_a_root_that_does_not_exist() {
    let workspace = workspace();
    let confinement =
        Confinement::new("/bin/cat", vec![], &workspace.root).with_roots(&ConfinementRequest {
            read_roots: vec![workspace.root.join("missing")],
            write_roots: vec![],
        });

    assert!(matches!(
        confinement.invocation(Some(Backend::Seatbelt)),
        Err(ConfinementError::UnusablePath { .. })
    ));
}

#[test]
fn test_confinement_rejects_a_relative_root() {
    let workspace = workspace();
    let confinement =
        Confinement::new("/bin/cat", vec![], &workspace.root).with_roots(&ConfinementRequest {
            read_roots: vec![PathBuf::from("relative/root")],
            write_roots: vec![],
        });

    assert!(matches!(
        confinement.invocation(Some(Backend::Seatbelt)),
        Err(ConfinementError::UnusablePath { .. })
    ));
}

fn bubblewrap_arguments(confinement: &Confinement) -> Vec<String> {
    let invocation = confinement.invocation(Some(Backend::Bubblewrap)).unwrap();
    assert_eq!(invocation.program, PathBuf::from("/usr/bin/bwrap"));
    assert!(
        invocation.environment.is_empty(),
        "bubblewrap carries the environment through --setenv"
    );
    invocation.arguments
}

fn window(arguments: &[String], values: &[&str]) -> bool {
    arguments
        .windows(values.len())
        .any(|slice| slice.iter().zip(values).all(|(left, right)| left == right))
}

#[test]
fn test_bubblewrap_arguments_unshare_everything() {
    let workspace = workspace();
    let arguments = bubblewrap_arguments(&confinement(&workspace.root, vec![]));

    assert_eq!(
        arguments[..11],
        [
            "--die-with-parent",
            "--new-session",
            "--unshare-all",
            "--unshare-net",
            "--clearenv",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
        ]
    );
}

#[test]
fn test_bubblewrap_arguments_bind_system_trees_read_only() {
    let workspace = workspace();
    let arguments = bubblewrap_arguments(&confinement(&workspace.root, vec![]));

    for tree in ["/usr", "/bin", "/lib", "/lib64"] {
        assert!(window(&arguments, &["--ro-bind-try", tree, tree]), "{tree}");
    }
    assert!(window(&arguments, &["--dir", "/etc"]));
    for file in ["/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/localtime"] {
        assert!(window(&arguments, &["--ro-bind-try", file, file]), "{file}");
    }
}

#[test]
fn test_bubblewrap_arguments_bind_the_requested_roots() {
    let workspace = workspace();
    let arguments = bubblewrap_arguments(&confinement(&workspace.root, vec![]));
    let root = text(&workspace.root);

    assert!(window(&arguments, &["--ro-bind", &root, &root]));
    assert!(window(&arguments, &["--bind", &root, &root]));
}

#[test]
fn test_bubblewrap_arguments_set_the_environment_and_working_directory() {
    let workspace = workspace();
    let arguments = bubblewrap_arguments(&confinement(&workspace.root, vec![]));
    let root = text(&workspace.root);

    for name in ["HOME", "TMPDIR", "TMP", "TEMP"] {
        assert!(window(&arguments, &["--setenv", name, &root]), "{name}");
    }
    assert!(window(&arguments, &["--setenv", "LANG", "C.UTF-8"]));
    assert!(window(&arguments, &["--setenv", "LC_ALL", "C.UTF-8"]));
    assert!(window(&arguments, &["--chdir", &root]));
}

#[test]
fn test_bubblewrap_arguments_end_with_the_command_and_its_arguments() {
    let workspace = workspace();
    let arguments = bubblewrap_arguments(&confinement(
        &workspace.root,
        vec![text(&workspace.granted)],
    ));
    let command = text(&fs::canonicalize("/bin/cat").unwrap());

    let separator = arguments.iter().position(|value| value == "--").unwrap();
    assert_eq!(arguments[separator + 1], command);
    assert_eq!(arguments[separator + 2], text(&workspace.granted));
    assert_eq!(arguments.len(), separator + 3);
}

#[test]
fn test_bubblewrap_arguments_keep_a_caller_supplied_environment() {
    let workspace = workspace();
    let confinement = confinement(&workspace.root, vec![])
        .with_environment(HashMap::from([("HOME".to_string(), "/tmp".to_string())]));
    let arguments = bubblewrap_arguments(&confinement);

    assert!(window(&arguments, &["--setenv", "HOME", "/tmp"]));
}

#[test]
fn test_unsupported_platform_yields_an_error_not_an_unconfined_command() {
    let workspace = workspace();
    let confinement = confinement(&workspace.root, vec![]);

    assert_eq!(
        confinement.invocation(None),
        Err(ConfinementError::UnsupportedPlatform)
    );
}

#[test]
fn test_capability_is_advertised_only_when_a_backend_exists() {
    let advertised = Capability::supported().contains(&"confinement".to_string());

    assert_eq!(advertised, Confinement::is_available());
    assert!(Capability::all().contains(&"confinement".to_string()));
}

#[test]
fn test_run_start_carries_a_confinement_request() {
    let json = r#"{
        "type": "RunStart",
        "job_id": "confined",
        "workspace": "/tmp",
        "command": "ls",
        "confinement": {
            "read_roots": ["/tmp/source"],
            "write_roots": ["/tmp/output"]
        }
    }"#;

    let message: InboundMessage = serde_json::from_str(json).unwrap();
    match message {
        InboundMessage::RunStart { confinement, .. } => {
            let confinement = confinement.unwrap();
            assert_eq!(confinement.read_roots, vec![PathBuf::from("/tmp/source")]);
            assert_eq!(confinement.write_roots, vec![PathBuf::from("/tmp/output")]);
        }
        _ => panic!("Wrong message type"),
    }
}

#[test]
fn test_run_start_without_confinement_stays_unconfined() {
    let json = r#"{"type": "RunStart", "job_id": "plain", "workspace": "/tmp", "command": "ls"}"#;
    let message: InboundMessage = serde_json::from_str(json).unwrap();

    match message {
        InboundMessage::RunStart { confinement, .. } => assert!(confinement.is_none()),
        _ => panic!("Wrong message type"),
    }
}

#[test]
fn test_confinement_unavailable_has_an_error_code() {
    let message = OutboundMessage::error(
        "job-1",
        ErrorCode::ConfinementUnavailable,
        "no backend on this host",
    );
    let json = serde_json::to_string(&message).unwrap();

    assert!(
        json.contains(r#""error_code":"confinement_unavailable""#),
        "{json}"
    );
}

#[tokio::test]
async fn test_spawn_fails_closed_when_confinement_cannot_be_established() {
    let workspace = workspace();
    let (tx, mut rx) = mpsc::channel(100);

    let request = InboundMessage::RunStart {
        job_id: "unprovable".to_string(),
        workspace: workspace.root.clone(),
        command: "/bin/cat".to_string(),
        args: vec![text(&workspace.granted)],
        env: HashMap::new(),
        timeout_ms: Some(15000),
        max_output_bytes: None,
        working_dir: None,
        confinement: Some(ConfinementRequest {
            read_roots: vec![workspace.root.join("does-not-exist")],
            write_roots: vec![],
        }),
    };

    let result = CommandExecutor::new().spawn(&request, tx).await;

    match result {
        Err(error @ ExecutorError::ConfinementUnavailable(_)) => {
            assert_eq!(error.to_error_code(), ErrorCode::ConfinementUnavailable);
        }
        Err(error) => panic!("Wrong error type: {error:?}"),
        Ok(_) => panic!("An unprovable confinement must not spawn the command"),
    }

    assert!(
        collect_messages(&mut rx, Duration::from_millis(200))
            .await
            .is_empty(),
        "A refused spawn must not report a started process"
    );
}

#[tokio::test]
async fn test_probe_result_is_cached() {
    let first = Confinement::probe().await;
    let second = Confinement::probe().await;

    assert_eq!(first, second);
}

async fn run_confined(request: &InboundMessage) -> Vec<OutboundMessage> {
    let (tx, mut rx) = mpsc::channel(1000);
    CommandExecutor::new().spawn(request, tx).await.unwrap();
    collect_messages(&mut rx, Duration::from_secs(20)).await
}

#[tokio::test]
async fn test_confined_command_reads_a_granted_root() {
    if Confinement::probe().await.is_err() {
        return;
    }

    let workspace = workspace();
    let messages = run_confined(&confined_run(
        "granted",
        &workspace.root,
        &workspace.granted,
    ))
    .await;

    assert_eq!(exit_code(&messages), Some(0), "{messages:?}");
    assert_eq!(stdout(&messages), GRANTED);
}

#[tokio::test]
async fn test_confined_command_cannot_read_outside_its_roots() {
    if Confinement::probe().await.is_err() {
        return;
    }

    let workspace = workspace();
    let messages = run_confined(&confined_run("denied", &workspace.root, &workspace.denied)).await;

    assert_ne!(exit_code(&messages), Some(0), "{messages:?}");
    assert!(
        !stdout(&messages).contains(SECRET.trim()),
        "the sandbox leaked a file outside its read roots"
    );
}

/// Point `client` at a fresh local listener and report whether the connection
/// arrived, plus the exit code the client reported.
async fn attempt_connection(root: &Path, client: &str, confined: bool) -> (bool, Option<i32>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let connected = Arc::new(AtomicBool::new(false));
    let accepted = connected.clone();
    let acceptor = tokio::spawn(async move {
        if listener.accept().await.is_ok() {
            accepted.store(true, Ordering::SeqCst);
        }
    });

    let args = if client.ends_with("curl") {
        vec![
            "--silent".to_string(),
            "--max-time".to_string(),
            "2".to_string(),
            format!("http://127.0.0.1:{port}/"),
        ]
    } else {
        vec![
            "-w".to_string(),
            "1".to_string(),
            "127.0.0.1".to_string(),
            port.to_string(),
        ]
    };

    let messages = run_confined(&InboundMessage::RunStart {
        job_id: format!("network-{confined}"),
        workspace: root.to_path_buf(),
        command: client.to_string(),
        args,
        env: HashMap::new(),
        timeout_ms: Some(15000),
        max_output_bytes: None,
        working_dir: None,
        confinement: confined.then(|| request(root)),
    })
    .await;

    tokio::time::sleep(Duration::from_millis(50)).await;
    acceptor.abort();

    (connected.load(Ordering::SeqCst), exit_code(&messages))
}

#[tokio::test]
async fn test_confinement_blocks_a_connection_that_otherwise_succeeds() {
    if Confinement::probe().await.is_err() {
        return;
    }
    let Some(client) = ["/usr/bin/nc", "/bin/nc", "/usr/bin/curl"]
        .into_iter()
        .find(|candidate| Path::new(candidate).is_file())
    else {
        return;
    };

    let workspace = workspace();

    let (unconfined, _) = attempt_connection(&workspace.root, client, false).await;
    assert!(
        unconfined,
        "the unconfined control never reached the listener, so this test proves nothing"
    );

    let (confined, code) = attempt_connection(&workspace.root, client, true).await;
    assert!(!confined, "a confined command reached a local TCP listener");
    assert_ne!(code, Some(0));
}
