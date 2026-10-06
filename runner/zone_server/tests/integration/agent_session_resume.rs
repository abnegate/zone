//! Whether a CLI session started under one login's home resumes under another once
//! `sessions::carry` has copied its file there. `sessions::portable` is set from these.
//!
//! Ignored because each spends real turns of a subscription and needs a sign-in Zone cannot make
//! for itself. Run by hand, from `runner/`:
//!
//! `CLAUDE_CODE_OAUTH_TOKEN=<token from claude setup-token> cargo test -p zone_server
//! --no-default-features --features zone_context/test-utils --test integration
//! -- --ignored --exact agent_session_resume::claude_session_resume_across_homes`
//!
//! `ZONE_SPIKE_CODEX_HOMES=<home a>:<home b> cargo test -p zone_server --no-default-features
//! --features zone_context/test-utils --test integration -- --ignored --exact
//! agent_session_resume::codex_session_resume_across_homes`, where each home was signed in with
//! `CODEX_HOME=<home> codex login`.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;
use uuid::Uuid;
use zone_core::llm::AgentKind;
use zone_server::services::agent::sessions;

const HOMES: &str = "ZONE_SPIKE_CODEX_HOMES";
const HOME_SEPARATOR: char = ':';
const CODEX_READ_ONLY: &str = r#"sandbox_mode="read-only""#;
const CODEX_WRITABLE: &str = "workspace-write";

fn required(variable: &str, instructions: &str) -> String {
    env::var(variable)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| panic!("{variable} is not set: {instructions}"))
}

fn nonce() -> String {
    format!("zephyr{}", &Uuid::new_v4().simple().to_string()[..8])
}

fn run(mut command: Command, prompt: &str) -> Output {
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("could not start {program}: {error}; is it on PATH?"));
    child
        .stdin
        .take()
        .expect("the child's stdin")
        .write_all(prompt.as_bytes())
        .expect("the prompt reaches the child");
    let output = child.wait_with_output().expect("the child exits");
    assert!(
        output.status.success(),
        "{program} failed ({}):\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn listing(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(directory) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            found.extend(listing(&path));
        } else {
            found.push(path);
        }
    }
    found
}

fn carry(from: &Path, to: &Path, agent: AgentKind, work: &Path, id: &str) {
    let carried = sessions::carry(from, to, agent, work, id).unwrap_or_else(|error| {
        panic!(
            "{agent} kept session {id} nowhere sessions::locate looks ({error}); {} holds {:#?}",
            from.display(),
            listing(from)
        )
    });
    assert!(carried.bytes > 0, "{agent}'s session file is empty");
}

fn claude(home: &Path, work: &Path, token: &str, session: [&str; 2], prompt: &str) -> String {
    let mut command = Command::new(AgentKind::Claude.executable());
    command
        .current_dir(work)
        .env(AgentKind::Claude.home(), home)
        .env("CLAUDE_CODE_OAUTH_TOKEN", token)
        .env_remove("ANTHROPIC_API_KEY")
        .envs(AgentKind::Claude.defaults().iter().copied())
        .args([
            "--output-format",
            "json",
            "--setting-sources",
            "",
            "--tools",
            "",
        ])
        .args(session)
        .arg("--print");
    let output = run(command, prompt);
    let result: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "claude printed no JSON result ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    assert_eq!(result["is_error"], false, "claude's turn failed: {result}");
    result["result"]
        .as_str()
        .unwrap_or_else(|| panic!("claude's result has no text: {result}"))
        .to_string()
}

struct CodexTurn {
    thread: Option<String>,
    answer: String,
}

fn codex(home: &Path, work: &Path, arguments: &[&str], prompt: &str) -> CodexTurn {
    let mut command = Command::new(AgentKind::Codex.executable());
    command
        .current_dir(work)
        .env(AgentKind::Codex.home(), home)
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .args([
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--ignore-user-config",
        ])
        .args(arguments);
    let output = run(command, prompt);

    let mut turn = CodexTurn {
        thread: None,
        answer: String::new(),
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event["type"].as_str() {
            Some("thread.started") => turn.thread = event["thread_id"].as_str().map(str::to_string),
            Some("item.completed") if event["item"]["type"] == "agent_message" => {
                if let Some(text) = event["item"]["text"].as_str() {
                    turn.answer.push_str(text);
                    turn.answer.push('\n');
                }
            }
            _ => {}
        }
    }
    turn
}

#[test]
#[ignore = "spends two turns of a real Claude subscription"]
fn claude_session_resume_across_homes() {
    let token = required(
        "CLAUDE_CODE_OAUTH_TOKEN",
        "set it to a token printed by `claude setup-token`",
    );
    let first = TempDir::new().expect("a first config directory");
    let second = TempDir::new().expect("a second config directory");
    let work = TempDir::new().expect("a work directory");
    let id = Uuid::new_v4().to_string();
    let word = nonce();

    claude(
        first.path(),
        work.path(),
        &token,
        ["--session-id", &id],
        &format!("Remember the word {word}. Reply with only OK."),
    );
    carry(
        first.path(),
        second.path(),
        AgentKind::Claude,
        work.path(),
        &id,
    );
    let answer = claude(
        second.path(),
        work.path(),
        &token,
        ["--resume", &id],
        "What was the word I asked you to remember? Reply with only that word.",
    );

    assert!(
        answer.contains(&word),
        "claude resumed under the second config directory without the first turn: {answer:?}"
    );
}

#[test]
#[ignore = "spends two turns of a real ChatGPT subscription"]
fn codex_session_resume_across_homes() {
    let homes = required(
        HOMES,
        "set it to two codex homes, `<a>:<b>`, each signed in with `CODEX_HOME=<home> codex login`",
    );
    let (first, second) = homes
        .split_once(HOME_SEPARATOR)
        .map(|(first, second)| (PathBuf::from(first), PathBuf::from(second)))
        .unwrap_or_else(|| panic!("{HOMES} names two homes separated by `{HOME_SEPARATOR}`"));
    for home in [&first, &second] {
        assert!(
            home.is_dir(),
            "{} is not a directory; sign it in with `CODEX_HOME={} codex login`",
            home.display(),
            home.display()
        );
    }
    assert_ne!(first, second, "{HOMES} names the same home twice");
    let work = TempDir::new().expect("a work directory");
    let word = nonce();
    let written = work.path().join(format!("{word}.txt"));

    let started = codex(
        &first,
        work.path(),
        &["--sandbox", CODEX_WRITABLE, "-"],
        &format!("Remember the word {word}. Reply with only OK."),
    );
    let id = started
        .thread
        .expect("codex announced no thread.started id on the first turn");
    carry(&first, &second, AgentKind::Codex, work.path(), &id);
    let resumed = codex(
        &second,
        work.path(),
        &["-c", CODEX_READ_ONLY, "resume", &id, "-"],
        &format!(
            "First, run a shell command that creates the file {} containing the word you were \
             asked to remember. Then reply with that word, whether or not the file was created.",
            written.display()
        ),
    );

    assert!(
        resumed.answer.contains(&word),
        "codex resumed under the second home without the first turn: {:?}",
        resumed.answer
    );
    assert!(
        !written.exists(),
        "the resumed turn wrote {} although it ran with {CODEX_READ_ONLY}",
        written.display()
    );
}
