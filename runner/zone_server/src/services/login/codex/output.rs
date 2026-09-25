//! What a short-lived codex or claude command printed, and how it exited.

use std::path::Path;
use std::process::ExitStatus;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tool_runner::executor::GRACE_PERIOD;

use super::Error;
use super::process::Process;
use super::prompt::strip;

pub(super) const KEPT: usize = 64 * 1024;
const READ: usize = 8 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl Output {
    pub(crate) async fn capture(command: Command, executable: &Path) -> Result<Self, Error> {
        Self::capture_within(command, executable, TIMEOUT).await
    }

    pub(crate) async fn capture_within(
        command: Command,
        executable: &Path,
        limit: Duration,
    ) -> Result<Self, Error> {
        let mut process = Process::spawn(command, executable)?;
        let stdout = tokio::spawn(read(process.stdout()));
        let stderr = tokio::spawn(read(process.stderr()));
        let status = match timeout(limit, process.wait()).await {
            Ok(Ok(status)) => status,
            Ok(Err(error)) => {
                process.stop().await;
                stdout.abort();
                stderr.abort();
                return Err(Error::Unreadable(format!(
                    "{} could not be waited for: {error}",
                    executable.display()
                )));
            }
            Err(_) => {
                process.stop().await;
                stdout.abort();
                stderr.abort();
                return Err(Error::Unreadable(format!(
                    "{} did not finish within {limit:?}",
                    executable.display()
                )));
            }
        };
        Ok(Self {
            status,
            stdout: strip(&collect(stdout).await).into_owned(),
            stderr: strip(&collect(stderr).await).into_owned(),
        })
    }

    pub(crate) fn failed(&self, executable: &Path) -> Error {
        failure(executable, &self.stderr, self.status)
    }
}

/// The CLI's own words for why it failed, or how it exited when it gave none.
pub(super) fn failure(executable: &Path, stderr: &str, status: ExitStatus) -> Error {
    let reason = strip(stderr);
    let reason = reason.trim();
    if reason.is_empty() {
        Error::Failed(format!(
            "{} exited with {status} and gave no reason",
            executable.display()
        ))
    } else {
        Error::Failed(reason.to_string())
    }
}

/// Everything `stream` carries until it closes, keeping the first [`KEPT`] bytes. It goes on
/// reading past that, because a child blocked on a full pipe never exits.
pub(super) async fn read(stream: Option<impl AsyncRead + Unpin>) -> String {
    let Some(mut stream) = stream else {
        return String::new();
    };
    let mut kept = Vec::new();
    let mut buffer = [0_u8; READ];
    while let Ok(read) = stream.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        let room = KEPT.saturating_sub(kept.len());
        kept.extend_from_slice(&buffer[..read.min(room)]);
    }
    String::from_utf8_lossy(&kept).into_owned()
}

/// What `reader` read, given a bounded wait: a grandchild that inherited the pipe can hold it open
/// long after the CLI itself has exited.
pub(super) async fn collect(mut reader: JoinHandle<String>) -> String {
    match timeout(GRACE_PERIOD, &mut reader).await {
        Ok(text) => text.unwrap_or_default(),
        Err(_) => {
            reader.abort();
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::services::login::codex::command;
    use crate::services::login::codex::testing::{ended, environment, fake, process};

    #[tokio::test]
    async fn a_command_that_never_finishes_is_stopped_at_its_limit() {
        let directory = TempDir::new().expect("a temporary directory");
        let record = directory.path().join("process");
        let cli = fake(
            &directory,
            "status",
            &format!("echo $$ > '{}'\nexec sleep 60", record.display()),
        );

        let error = Output::capture_within(
            command(&cli, &["status"], &environment()),
            &cli,
            Duration::from_secs(1),
        )
        .await
        .expect_err("a command that never finishes");

        assert!(
            matches!(&error, Error::Unreadable(message) if message.contains("did not finish")),
            "{error:?}"
        );
        assert!(
            ended(process(&record)).await,
            "the command was left running"
        );
    }

    #[tokio::test]
    async fn output_past_what_is_kept_is_still_read_so_the_command_can_exit() {
        let directory = TempDir::new().expect("a temporary directory");
        let cli = fake(
            &directory,
            "status",
            "head -c 1048576 /dev/zero | tr '\\0' 'x'\necho 'done' >&2\nexit 3",
        );

        let output = Output::capture_within(
            command(&cli, &["status"], &environment()),
            &cli,
            Duration::from_secs(20),
        )
        .await
        .expect("the command's output");

        assert_eq!(output.stdout.len(), KEPT);
        assert_eq!(output.stderr, "done\n");
        assert_eq!(output.status.code(), Some(3));
    }

    #[tokio::test]
    async fn colour_codes_never_reach_what_was_captured() {
        let directory = TempDir::new().expect("a temporary directory");
        let cli = fake(
            &directory,
            "status",
            "printf '\\033[94mLogged in using ChatGPT\\033[0m\\n' >&2",
        );

        let output = Output::capture_within(
            command(&cli, &["status"], &environment()),
            &cli,
            Duration::from_secs(20),
        )
        .await
        .expect("the command's output");

        assert_eq!(output.stderr, "Logged in using ChatGPT\n");
    }

    #[tokio::test]
    async fn a_missing_cli_is_unavailable() {
        let missing = Path::new("/nonexistent/zone/codex");

        let error = Output::capture_within(
            command(missing, &["status"], &environment()),
            missing,
            Duration::from_secs(20),
        )
        .await
        .expect_err("a missing CLI");

        assert!(
            matches!(&error, Error::Unavailable { executable, .. } if executable == "/nonexistent/zone/codex"),
            "{error:?}"
        );
    }
}
