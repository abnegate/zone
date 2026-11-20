//! Git as the worktree sweep runs it: in a folder the workspace mounted, owned by the host's
//! user rather than Zone's, with no system or global configuration, no filesystem monitor, and
//! no lazy fetch of an object a partial clone lacks.

use std::io;
use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Longest one git command may run. Nothing here reaches the network, so this only bounds a
/// command wedged on a very large repository.
const TIMEOUT: Duration = Duration::from_secs(120);

/// Variables that would point git at a repository other than the directory it runs in.
const REPOSITORY_ENVIRONMENT: [&str; 4] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
];

const PATCH_ID: [&str; 2] = ["patch-id", "--verbatim"];

fn command(directory: &Path) -> Command {
    let mut command = Command::new("git");
    for variable in REPOSITORY_ENVIRONMENT {
        command.env_remove(variable);
    }
    command
        .args(["-c", "safe.directory=*", "-c", "core.fsmonitor=false"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(directory)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    command
}

pub async fn output(directory: &Path, arguments: &[&str]) -> io::Result<Output> {
    let output = command(directory)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    tokio::time::timeout(TIMEOUT, output)
        .await
        .map_err(|_| timed_out(arguments))?
}

/// Standard output, trimmed, of a command that has to succeed.
pub async fn read(directory: &Path, arguments: &[&str]) -> io::Result<String> {
    let output = output(directory, arguments).await?;
    if !output.status.success() {
        return Err(failed(arguments, &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Whether a command that answers a question by its exit status said yes (0) or no (1).
pub async fn answers(directory: &Path, arguments: &[&str]) -> io::Result<bool> {
    let output = output(directory, arguments).await?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(failed(arguments, &output)),
    }
}

/// The verbatim patch id of every patch `arguments` prints, in order, streamed through
/// `git patch-id` so a long history is never held in memory. Verbatim, because a change that
/// differs only in whitespace is still a different change.
pub async fn patch_ids(directory: &Path, arguments: &[&str]) -> io::Result<Vec<String>> {
    let mut producer = command(directory)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut complaint = producer
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("git kept no standard error to read"))?;
    let patches: Stdio = producer
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("git kept no standard output to read"))?
        .try_into()?;
    let consumer = command(directory)
        .args(PATCH_ID)
        .stdin(patches)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut diagnostic = Vec::new();
    let (produced, consumed, _) = tokio::time::timeout(TIMEOUT, async {
        tokio::join!(
            producer.wait(),
            consumer.wait_with_output(),
            complaint.read_to_end(&mut diagnostic)
        )
    })
    .await
    .map_err(|_| timed_out(arguments))?;
    if !produced?.success() {
        return Err(io::Error::other(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&diagnostic).trim()
        )));
    }
    let consumed = consumed?;
    if !consumed.status.success() {
        return Err(failed(&PATCH_ID, &consumed));
    }
    Ok(String::from_utf8_lossy(&consumed.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .collect())
}

fn failed(arguments: &[&str], output: &Output) -> io::Error {
    io::Error::other(format!(
        "git {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn timed_out(arguments: &[&str]) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "git {} ran past {}s",
            arguments.join(" "),
            TIMEOUT.as_secs()
        ),
    )
}
