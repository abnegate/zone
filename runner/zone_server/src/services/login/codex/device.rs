//! A pending `codex login --device-auth`: the prompt it printed, and the task that sees it through.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use tokio::io::AsyncReadExt;
use tokio::process::ChildStdout;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until, timeout_at};
use zone_core::llm::provider::Lines;

use super::output::{self, collect, failure};
use super::process::Process;
use super::prompt::{NO_LINK, strip};
use super::staging::Staging;
use super::{Error, HOME, LOGIN, Limits, Prompt, command};

const LINE: usize = 4 * 1024;
const PRINTED: usize = 64 * 1024;
const READ: usize = 8 * 1024;
const CANCELLED: &str = "The sign-in was cancelled";

#[derive(Debug)]
pub struct Device {
    pub prompt: Prompt,
    /// Resolves once codex has exited or been stopped and its staging directory is gone. `Ok` only
    /// when the new login has replaced the organization's.
    pub outcome: JoinHandle<Result<(), Error>>,
    /// Sending on it, or dropping it, stops codex and discards the attempt.
    pub cancel: oneshot::Sender<()>,
}

impl Device {
    pub(super) async fn start(
        executable: &Path,
        home: &Path,
        environment: &BTreeMap<String, String>,
        limits: Limits,
    ) -> Result<Self, Error> {
        let staging = Staging::create(home)?;
        let mut command = command(executable, LOGIN, environment);
        command.env(HOME, staging.path());
        let mut process = Process::spawn(command, executable)?;
        let stderr = tokio::spawn(output::read(process.stderr()));
        let mut stdout = process
            .stdout()
            .ok_or_else(|| Error::Unreadable("codex's output could not be read".to_string()))?;
        let deadline = Instant::now() + limits.prompt;

        let prompt = match read(&mut stdout, deadline, limits.prompt).await {
            Ok(Some(prompt)) => prompt,
            Ok(None) => return Err(ended(&mut process, stderr, deadline, executable).await),
            Err(error) => {
                process.stop().await;
                stderr.abort();
                return Err(error);
            }
        };

        let lifetime = (prompt.expires_at - Utc::now())
            .to_std()
            .unwrap_or(Duration::ZERO);
        let Some(expiry) = Instant::now().checked_add(lifetime.saturating_add(limits.grace)) else {
            process.stop().await;
            stderr.abort();
            return Err(Error::Unreadable(
                "codex printed an expiry too far away to wait for".to_string(),
            ));
        };
        let (cancel, cancelled) = oneshot::channel();
        let outcome = tokio::spawn(watch(
            process,
            stdout,
            stderr,
            staging,
            cancelled,
            expiry,
            executable.to_path_buf(),
        ));
        Ok(Self {
            prompt,
            outcome,
            cancel,
        })
    }
}

/// Codex's stdout, read until it holds a whole prompt. `Ok(None)` means codex closed it first,
/// which it does only on its way out.
async fn read(
    stdout: &mut ChildStdout,
    deadline: Instant,
    limit: Duration,
) -> Result<Option<Prompt>, Error> {
    let mut lines = Lines::new(LINE);
    let mut text = String::new();
    let mut buffer = [0_u8; READ];
    let mut missing = NO_LINK;

    loop {
        let read = match timeout_at(deadline, stdout.read(&mut buffer)).await {
            Ok(Ok(read)) => read,
            Ok(Err(error)) => {
                return Err(Error::Unreadable(format!(
                    "codex's output could not be read: {error}"
                )));
            }
            Err(_) => {
                return Err(Error::Unreadable(format!(
                    "codex printed {missing} within {limit:?}"
                )));
            }
        };
        if read == 0 {
            if let Ok(Some(line)) = lines.flush() {
                append(&mut text, &line);
            }
            return Ok(Prompt::read(&text, Utc::now()).ok());
        }
        lines.extend(&buffer[..read]);
        while let Some(line) = lines.take().map_err(|overlong| {
            Error::Unreadable(format!(
                "codex printed a line longer than {} bytes instead of a sign-in prompt",
                overlong.limit
            ))
        })? {
            append(&mut text, &line);
        }
        if text.len() > PRINTED {
            return Err(Error::Unreadable(format!(
                "codex printed more than {PRINTED} bytes without a sign-in prompt"
            )));
        }
        match Prompt::read(&text, Utc::now()) {
            Ok(prompt) => return Ok(Some(prompt)),
            Err(part) => missing = part,
        }
    }
}

fn append(text: &mut String, line: &str) {
    text.push_str(&strip(line));
    text.push('\n');
}

/// Why codex exited before it printed a prompt, in its own words when it gave any.
async fn ended(
    process: &mut Process,
    stderr: JoinHandle<String>,
    deadline: Instant,
    executable: &Path,
) -> Error {
    match timeout_at(deadline, process.wait()).await {
        Ok(Ok(status)) => failure(executable, &collect(stderr).await, status),
        Ok(Err(error)) => {
            process.stop().await;
            stderr.abort();
            Error::Unreadable(format!("codex could not be waited for: {error}"))
        }
        Err(_) => {
            process.stop().await;
            stderr.abort();
            Error::Unreadable(
                "codex closed its output without printing a sign-in prompt".to_string(),
            )
        }
    }
}

/// Sees a pending sign-in through: codex exits, the caller cancels, or the code expires.
async fn watch(
    mut process: Process,
    stdout: ChildStdout,
    stderr: JoinHandle<String>,
    staging: Staging,
    cancelled: oneshot::Receiver<()>,
    expiry: Instant,
    executable: PathBuf,
) -> Result<(), Error> {
    let drain = tokio::spawn(drain(stdout));
    let outcome = tokio::select! {
        biased;
        _ = cancelled => {
            process.stop().await;
            stderr.abort();
            Err(Error::Failed(CANCELLED.to_string()))
        }
        () = sleep_until(expiry) => {
            process.stop().await;
            stderr.abort();
            Err(Error::Expired)
        }
        status = process.wait() => match status {
            Ok(status) if status.success() => {
                stderr.abort();
                staging.promote()
            }
            Ok(status) => Err(failure(&executable, &collect(stderr).await, status)),
            Err(error) => {
                stderr.abort();
                Err(Error::Failed(format!("codex could not be waited for: {error}")))
            }
        },
    };
    drain.abort();
    outcome
}

/// Codex prints nothing more once its prompt is out, but a child blocked on a full pipe never
/// exits, so whatever it does print is read and dropped.
async fn drain(mut stdout: ChildStdout) {
    let mut buffer = [0_u8; READ];
    while let Ok(read) = stdout.read(&mut buffer).await {
        if read == 0 {
            break;
        }
    }
}
