//! Background shell jobs: the shapes a spawn is reported in, and the limits
//! one runs under.
//!
//! A job outlives the tool call that started it but not the process that owns
//! it, so nothing here is persisted. The spawn receipt is also the only channel
//! a tool has to the chat layer — `ToolResult` carries no structured detail — so
//! the text is built and read back in this one file, and a round-trip test
//! keeps the two ends honest.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

pub const TAIL_JOB: &str = "tail_job";

/// `wait_for` is registered by `zone_server`, which depends on this crate, so
/// its name is spelled here rather than shared through a constant.
const WAIT_FOR_TOOL: &str = "wait_for";

/// Longest a background job may run before it is killed.
///
/// The same cap a foreground shell command is held to: backgrounding is a way
/// to stop blocking the loop, not a way to buy a longer command.
pub const MAX_JOB_LIFETIME: Duration = Duration::from_secs(super::command::MAX_SHELL_TIMEOUT_SECS);

/// Ceiling on a job's log file, past which the job is killed and reported as
/// flooded rather than truncated and reported as fine.
pub const MAX_JOB_LOG_BYTES: u64 = 64 * 1024 * 1024;

/// Where job logs live, relative to the session's own working directory.
///
/// Inside the checkout rather than a shared temp directory: `run_command`'s
/// allow-list reaches `cat`, `tail` and `grep` with unconstrained paths, so a
/// shared location would be a new cross-workspace read surface.
pub const JOB_LOG_DIRECTORY: &str = ".zone/jobs";

const JOB_LOG_EXTENSION: &str = "log";

/// Distinguishes a job id from a run id at a glance, and keeps it short enough
/// to carry between calls.
const JOB_ID_PREFIX: &str = "job_";
const JOB_ID_HEX_CHARS: usize = 12;

const STARTED_PREFIX: &str = "Started ";

/// A background job, as reported to the console and read back by the chat layer
/// from the spawn receipt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobStarted {
    pub id: String,
    pub pid: u32,
    pub log_path: String,
}

/// A background job that is no longer running. No exit code means it was killed
/// rather than allowed to finish.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobExited {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// Mint a job id: `job_` and twelve lowercase hex characters.
pub fn mint() -> String {
    let hex = Uuid::new_v4().simple().to_string();
    format!("{JOB_ID_PREFIX}{}", &hex[..JOB_ID_HEX_CHARS])
}

/// Where the log for `id` belongs, under the session's working directory.
pub fn log_path(cwd: &Path, id: &str) -> PathBuf {
    cwd.join(JOB_LOG_DIRECTORY)
        .join(format!("{id}.{JOB_LOG_EXTENSION}"))
}

/// What a backgrounded shell call returns to the model.
pub fn started_text(job: &JobStarted) -> String {
    format!(
        "{STARTED_PREFIX}{} (pid {}). Log: {}\nWait for it with {WAIT_FOR_TOOL}, or read it with {TAIL_JOB}.",
        job.id, job.pid, job.log_path
    )
}

/// Read a job id back out of a spawn receipt, for a caller holding only the
/// tool's own output.
pub fn parse_started(output: &str) -> Option<String> {
    output
        .lines()
        .next()?
        .strip_prefix(STARTED_PREFIX)?
        .split_whitespace()
        .next()
        .filter(|candidate| is_job_id(candidate))
        .map(str::to_string)
}

fn is_job_id(candidate: &str) -> bool {
    candidate.strip_prefix(JOB_ID_PREFIX).is_some_and(|hex| {
        hex.len() == JOB_ID_HEX_CHARS
            && hex
                .chars()
                .all(|character| matches!(character, '0'..='9' | 'a'..='f'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> JobStarted {
        JobStarted {
            id: "job_9f3c1a7b2e04".to_string(),
            pid: 48213,
            log_path: "/tmp/work/.zone/jobs/job_9f3c1a7b2e04.log".to_string(),
        }
    }

    #[test]
    fn a_spawn_receipt_reads_back_as_the_job_it_announced() {
        let job = job();
        assert_eq!(parse_started(&started_text(&job)), Some(job.id.clone()));
    }

    #[test]
    fn a_spawn_receipt_names_the_job_the_pid_and_both_follow_up_tools() {
        assert_eq!(
            started_text(&job()),
            "Started job_9f3c1a7b2e04 (pid 48213). Log: /tmp/work/.zone/jobs/job_9f3c1a7b2e04.log\n\
             Wait for it with wait_for, or read it with tail_job."
        );
    }

    #[test]
    fn output_that_announced_no_job_parses_as_none() {
        assert_eq!(parse_started(""), None);
        assert_eq!(parse_started("total 0\n"), None);
        assert_eq!(
            parse_started("Started run_42 (pid 1)."),
            None,
            "only an id in the minted shape is a job id"
        );
        assert_eq!(
            parse_started("Started job_9F3C1A7B2E04 (pid 1)."),
            None,
            "job ids are lowercase hex"
        );
    }

    #[test]
    fn a_minted_id_is_the_shape_the_parser_accepts() {
        let id = mint();
        assert!(id.starts_with(JOB_ID_PREFIX), "{id}");
        assert_eq!(id.len(), JOB_ID_PREFIX.len() + JOB_ID_HEX_CHARS, "{id}");
        assert_ne!(id, mint(), "each job gets its own id");

        let started = JobStarted {
            id: id.clone(),
            pid: 1,
            log_path: "/tmp/x.log".to_string(),
        };
        assert_eq!(parse_started(&started_text(&started)), Some(id));
    }

    #[test]
    fn a_log_lives_under_the_session_working_directory() {
        assert_eq!(
            log_path(Path::new("/tmp/work"), "job_9f3c1a7b2e04"),
            PathBuf::from("/tmp/work/.zone/jobs/job_9f3c1a7b2e04.log")
        );
    }

    #[test]
    fn a_job_that_was_killed_serialises_without_an_exit_code() {
        let killed = JobExited {
            id: "job_9f3c1a7b2e04".to_string(),
            exit_code: None,
        };
        let json = serde_json::to_value(&killed).unwrap();
        assert!(json.get("exit_code").is_none(), "{json}");
        assert_eq!(
            serde_json::from_value::<JobExited>(json).unwrap(),
            killed,
            "a killed job round-trips without inventing an exit code"
        );
    }
}
