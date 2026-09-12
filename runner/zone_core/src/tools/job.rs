//! Background shell jobs: the registry that owns every child process this
//! server started, the shapes a spawn is reported in, and the limits one runs
//! under.
//!
//! A job outlives the tool call that started it but not the process that owns
//! it, so nothing here is persisted: a row that survived a restart the child
//! did not would claim a durability the thing has never had. The same
//! single-instance assumption the run socket and the question waiter already
//! document holds here.
//!
//! The spawn receipt is also the only channel a tool has to the chat layer —
//! `ToolResult` carries no structured detail — so the text is built and read
//! back in this one file, and a round-trip test keeps the two ends honest.

use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, watch};
use tool_runner::Proxy;
use uuid::Uuid;

use super::Session;

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

/// What a context with no chat and no run is told when it reaches for a job.
pub const UNAVAILABLE: &str = "Background jobs are not available in this context.";

const JOB_LOG_EXTENSION: &str = "log";

/// Distinguishes a job id from a run id at a glance, and keeps it short enough
/// to carry between calls.
const JOB_ID_PREFIX: &str = "job_";
const JOB_ID_HEX_CHARS: usize = 12;

const STARTED_PREFIX: &str = "Started ";

const SHELL: &str = "sh";
const SHELL_COMMAND_FLAG: &str = "-c";

/// How often a running job's log is measured against its ceiling.
///
/// Measuring costs one `stat` and the ceiling is there to protect the disk, so
/// the interval is short enough that even a pathological writer puts little
/// past it before the kill lands.
const LOG_CHECK_INTERVAL: Duration = Duration::from_millis(250);

/// Widest a single UTF-8 character is, and so the most a read holds back while
/// waiting for the rest of one.
const MAX_CHARACTER_BYTES: usize = 4;

/// The line that keeps a task run's job logs out of its diff.
const EXCLUDED: &str = ".zone/";

/// Asked of git rather than joined onto `.git`: in a linked worktree `.git` is
/// a pointer file and the real exclude lives in the common directory.
const EXCLUDE_PATH: &str = "info/exclude";

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

/// Where a job has got to, spelled the way `tail_job` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Exited(i32),
    Killed,
    Flooded,
}

impl fmt::Display for JobState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => formatter.write_str("running"),
            Self::Exited(code) => write!(formatter, "exited {code}"),
            Self::Killed => formatter.write_str("killed"),
            Self::Flooded => formatter.write_str("flooded"),
        }
    }
}

impl JobState {
    pub fn settled(self) -> bool {
        !matches!(self, Self::Running)
    }

    /// A job that stopped without an exit code was stopped by us.
    fn exited(self, id: String) -> JobExited {
        JobExited {
            id,
            exit_code: match self {
                Self::Exited(code) => Some(code),
                _ => None,
            },
        }
    }
}

/// What a background job runs, and where.
///
/// A program and its arguments rather than a shell line, so that backgrounding
/// keeps `run_command`'s allow-list and metacharacter checks meaning what they
/// mean in the foreground: an argument inspected whole must not be word-split
/// on its way to a child.
///
/// The directory is the child's, and carried here rather than passed beside
/// the session's own tree so that the only path [`Jobs::spawn`] takes is the
/// tree it keys a job's log to. A caller naming a directory the model chose
/// can move the child and nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct JobCommand {
    pub program: String,
    pub arguments: Vec<String>,
    pub directory: Option<PathBuf>,
}

impl JobCommand {
    /// A shell line, as `run_shell` takes it.
    pub fn shell(line: impl Into<String>) -> Self {
        Self {
            program: SHELL.to_string(),
            arguments: vec![SHELL_COMMAND_FLAG.to_string(), line.into()],
            directory: None,
        }
    }

    /// A program and its arguments, as `run_command` takes them.
    pub fn new(program: impl Into<String>, arguments: Vec<String>) -> Self {
        Self {
            program: program.into(),
            arguments,
            directory: None,
        }
    }

    /// Run the child somewhere other than the session's own working tree.
    pub fn within(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directory = Some(directory.into());
        self
    }
}

/// A slice of a job's log, and where the next one starts.
#[derive(Debug, Clone, PartialEq)]
pub struct JobTail {
    pub output: String,
    pub state: JobState,
    pub next: u64,
}

/// One entry in [`JOBS`].
struct Job {
    session: Session,
    log: PathBuf,
    state: watch::Receiver<JobState>,
    kill: oneshot::Sender<()>,
}

/// What a job is held to.
///
/// A value rather than the constants read directly, so a test can prove the
/// reaper without writing [`MAX_JOB_LOG_BYTES`] to disk or waiting out
/// [`MAX_JOB_LIFETIME`].
#[derive(Debug, Clone, Copy)]
struct Limits {
    lifetime: Duration,
    log_bytes: u64,
    log_check: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            lifetime: MAX_JOB_LIFETIME,
            log_bytes: MAX_JOB_LOG_BYTES,
            log_check: LOG_CHECK_INTERVAL,
        }
    }
}

static JOBS: Lazy<DashMap<String, Job>> = Lazy::new(DashMap::new);

/// Every background child process this server owns.
pub struct Jobs;

impl Jobs {
    /// Start a job, and hand back the receipt the model is shown.
    ///
    /// `checkout` is the session's own working tree, and it is the only thing
    /// the log directory and the exclude write are ever derived from. Where the
    /// child runs is [`JobCommand::within`]'s to say, because that is the part
    /// a model chooses.
    pub async fn spawn(
        session: Session,
        command: &JobCommand,
        checkout: &Path,
        env: &HashMap<String, String>,
    ) -> Result<JobStarted, String> {
        Self::start(session, command, checkout, env, Limits::default()).await
    }

    /// Read a job's log from `since`, and say where the job has got to.
    ///
    /// The state is taken before the log rather than after, so a settled state
    /// never describes a slice read before the child's last write.
    pub async fn read(
        session: Session,
        id: &str,
        since: u64,
        max_chars: usize,
    ) -> Result<JobTail, String> {
        if session == Session::Detached {
            return Err(UNAVAILABLE.to_string());
        }
        let (log, state) = {
            let job = JOBS
                .get(id)
                .filter(|job| job.session == session)
                .ok_or_else(|| missing(id))?;
            (job.log.clone(), *job.state.borrow())
        };
        let unread = JobTail {
            output: String::new(),
            state,
            next: since,
        };

        let Ok(mut file) = tokio::fs::File::open(&log).await else {
            return Ok(unread);
        };
        if file.seek(SeekFrom::Start(since)).await.is_err() {
            return Ok(unread);
        }
        let mut buffer = Vec::with_capacity(max_chars);
        if file
            .take(max_chars as u64)
            .read_to_end(&mut buffer)
            .await
            .is_err()
        {
            return Ok(unread);
        }

        let (output, consumed) = decode(&buffer, state.settled());
        Ok(JobTail {
            output,
            state,
            next: since + consumed as u64,
        })
    }

    /// Claim a job's ending, to be awaited later.
    ///
    /// The claim is taken synchronously and the outcome is held in the job's
    /// own channel, so a job that ended before the caller got here resolves the
    /// future at once instead of stranding it.
    pub fn settled(
        session: Session,
        id: &str,
    ) -> Result<impl Future<Output = JobExited> + use<>, String> {
        if session == Session::Detached {
            return Err(UNAVAILABLE.to_string());
        }
        let state = {
            let job = JOBS
                .get(id)
                .filter(|job| job.session == session)
                .ok_or_else(|| missing(id))?;
            job.state.clone()
        };
        let id = id.to_string();
        Ok(async move { ended(state).await.exited(id) })
    }

    /// End every job this session started, take its log with it, and wait for
    /// each child to go.
    ///
    /// A job belongs to the turn or the run that started it, so this is the
    /// last thing either one does. Returns how many jobs it ended.
    pub async fn kill_session(session: Session) -> usize {
        let ids: Vec<String> = JOBS
            .iter()
            .filter(|job| job.session == session)
            .map(|job| job.key().clone())
            .collect();
        let claimed: Vec<Job> = ids
            .iter()
            .filter_map(|id| JOBS.remove(id).map(|(_, job)| job))
            .collect();

        let count = claimed.len();
        for job in claimed {
            let log = job.log;
            let _ = job.kill.send(());
            ended(job.state).await;
            discard(&log).await;
        }
        count
    }

    /// The entry is published before the supervisor starts, so a child that
    /// exits immediately still has somewhere to record that it did — the
    /// claim-before-publish ordering the question waiter documents.
    async fn start(
        session: Session,
        command: &JobCommand,
        checkout: &Path,
        env: &HashMap<String, String>,
        limits: Limits,
    ) -> Result<JobStarted, String> {
        if session == Session::Detached {
            return Err(UNAVAILABLE.to_string());
        }

        tokio::fs::create_dir_all(checkout.join(JOB_LOG_DIRECTORY))
            .await
            .map_err(|error| format!("Cannot create {JOB_LOG_DIRECTORY}: {error}"))?;
        if matches!(session, Session::Task(_)) {
            exclude(checkout).await;
        }

        let id = mint();
        let log = log_path(checkout, &id);
        let file = std::fs::File::create(&log)
            .map_err(|error| format!("Cannot create the job log: {error}"))?;
        let errors = file
            .try_clone()
            .map_err(|error| format!("Cannot create the job log: {error}"))?;

        let mut process = Command::new(&command.program);
        process
            .args(&command.arguments)
            .current_dir(command.directory.as_deref().unwrap_or(checkout))
            .stdin(Stdio::null())
            .stdout(Stdio::from(file))
            .stderr(Stdio::from(errors))
            .kill_on_drop(true);
        process.env_clear();
        for (key, value) in env {
            process.env(key, value);
        }
        Proxy::from_env().apply(&mut process);

        let child = process
            .spawn()
            .map_err(|error| format!("Failed to start the job: {error}"))?;
        let pid = child.id().unwrap_or_default();

        let (sender, state) = watch::channel(JobState::Running);
        let (kill, killed) = oneshot::channel();
        JOBS.insert(
            id.clone(),
            Job {
                session,
                log: log.clone(),
                state,
                kill,
            },
        );
        tokio::spawn(supervise(child, log.clone(), limits, killed, sender));

        Ok(JobStarted {
            id,
            pid,
            log_path: log.to_string_lossy().into_owned(),
        })
    }
}

/// Hold a job to its limits, and record how it ended.
///
/// Polled in order, not at random: a child that exits in the same tick as its
/// deadline elapses has exited, and reporting that as a kill would hand the
/// model a failure it did not have.
async fn supervise(
    mut child: Child,
    log: PathBuf,
    limits: Limits,
    kill: oneshot::Receiver<()>,
    state: watch::Sender<JobState>,
) {
    let outcome = {
        let flood = flooded(&log, limits.log_bytes, limits.log_check);
        tokio::select! {
            biased;
            status = child.wait() => match status {
                Ok(status) => status.code().map_or(JobState::Killed, JobState::Exited),
                Err(_) => JobState::Killed,
            },
            _ = kill => JobState::Killed,
            _ = tokio::time::sleep(limits.lifetime) => JobState::Killed,
            _ = flood => JobState::Flooded,
        }
    };
    if !matches!(outcome, JobState::Exited(_)) {
        let _ = child.kill().await;
    }
    state.send_replace(outcome);
}

/// Resolves once the log has passed the ceiling it is allowed.
async fn flooded(log: &Path, ceiling: u64, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        if tokio::fs::metadata(log)
            .await
            .is_ok_and(|log| log.len() > ceiling)
        {
            return;
        }
    }
}

/// Take a job's log with it, and the directories it needed once they are
/// empty.
///
/// Nothing can read the log after this: the registry entry it was reached
/// through is already gone, and a chat's logs sit in the operator's own
/// checkout, where the exclude write is skipped by design. A job the reaper
/// killed for its lifetime or for flooding keeps its log until here, so a
/// `tail_job` in the meantime still reports how it ended.
///
/// `remove_dir` on a directory something else is using fails, which is the
/// whole of the emptiness check.
async fn discard(log: &Path) {
    let _ = tokio::fs::remove_file(log).await;
    let Some(jobs) = log.parent() else {
        return;
    };
    if tokio::fs::remove_dir(jobs).await.is_err() {
        return;
    }
    if let Some(zone) = jobs.parent() {
        let _ = tokio::fs::remove_dir(zone).await;
    }
}

/// Resolves once the job has stopped, immediately if it already had.
async fn ended(mut state: watch::Receiver<JobState>) -> JobState {
    loop {
        let current = *state.borrow_and_update();
        if current.settled() {
            return current;
        }
        if state.changed().await.is_err() {
            return JobState::Killed;
        }
    }
}

/// Keep job logs out of a task run's diff.
///
/// Task runs only, and only in the run's own checkout: a chat works in the
/// host checkout, whose `.git` may be a pointer into a directory every
/// worktree of the repository shares, and one chat's job must not write into
/// all of them. It is also why the directory asked about is the session's own
/// tree and never one the command named — a run naming somebody else's
/// checkout would otherwise write into a repository it does not own. Every
/// failure — not a checkout, no git, an unwritable file — is a silent skip,
/// because a background job is worth more to the caller than a tidy diff.
async fn exclude(checkout: &Path) {
    let Ok(resolved) = Command::new("git")
        .arg("rev-parse")
        .arg("--git-path")
        .arg(EXCLUDE_PATH)
        .current_dir(checkout)
        .stdin(Stdio::null())
        .output()
        .await
    else {
        return;
    };
    if !resolved.status.success() {
        return;
    }
    let Ok(resolved) = std::str::from_utf8(&resolved.stdout) else {
        return;
    };
    let path = checkout.join(resolved.trim());

    let existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    if existing.lines().any(|line| line.trim() == EXCLUDED) {
        return;
    }
    let opening = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let Ok(mut file) = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
    else {
        return;
    };
    let _ = file
        .write_all(format!("{opening}{EXCLUDED}\n").as_bytes())
        .await;
}

/// Take whole characters only, and say how many bytes that spent.
///
/// A character split by the read budget is left for the next read rather than
/// replaced here, so a cursor walked across several reads loses nothing. Once
/// the job has stopped no more bytes are coming, so a trailing fragment is
/// spent rather than waited on forever.
fn decode(buffer: &[u8], settled: bool) -> (String, usize) {
    match std::str::from_utf8(buffer) {
        Ok(text) => (text.to_string(), buffer.len()),
        Err(error) => {
            let whole = error.valid_up_to();
            if whole == 0 && (settled || buffer.len() > MAX_CHARACTER_BYTES) {
                return (String::from_utf8_lossy(buffer).into_owned(), buffer.len());
            }
            (
                String::from_utf8_lossy(&buffer[..whole]).into_owned(),
                whole,
            )
        }
    }
}

fn missing(id: &str) -> String {
    format!("No job {id} in this session.")
}

/// Mint a job id: `job_` and twelve lowercase hex characters.
pub fn mint() -> String {
    let hex = Uuid::new_v4().simple().to_string();
    format!("{JOB_ID_PREFIX}{}", &hex[..JOB_ID_HEX_CHARS])
}

/// Where the log for `id` belongs, under the session's own working tree.
pub fn log_path(checkout: &Path, id: &str) -> PathBuf {
    checkout
        .join(JOB_LOG_DIRECTORY)
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

/// Where a spawn receipt keeps the pid, either side of it.
const RECEIPT_PID_OPENING: &str = " (pid ";
const RECEIPT_PID_CLOSING: &str = "). Log: ";

/// Read a whole job back out of a spawn receipt.
///
/// A caller holding only the tool's own output has nowhere else to look: the
/// registry keeps no pid and a `ToolResult` has no slot for one. What is read
/// back is therefore checked by rebuilding the receipt from it, so a change to
/// [`started_text`] stops this recognising the line rather than reporting a job
/// with the wrong pid. Reading lives beside writing for the same reason: the
/// format is this module's, and a reader that re-derived it elsewhere would
/// drift from the builder in silence.
pub fn parse_receipt(output: &str) -> Option<JobStarted> {
    let id = parse_started(output)?;
    let (announced, rest) = output.lines().next()?.split_once(RECEIPT_PID_OPENING)?;
    if !announced.ends_with(&id) {
        return None;
    }
    let (pid, log_path) = rest.split_once(RECEIPT_PID_CLOSING)?;
    let job = JobStarted {
        id,
        pid: pid.parse().ok()?,
        log_path: log_path.to_string(),
    };
    (started_text(&job) == output).then_some(job)
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
    use std::process::Command as Process;
    use tempfile::TempDir;

    const POLL: Duration = Duration::from_millis(20);
    const POLL_LIMIT: usize = 500;

    /// The directory [`JOB_LOG_DIRECTORY`] sits in, which a teardown takes too
    /// when the logs were the only thing in it.
    fn zone_directory() -> &'static Path {
        Path::new(JOB_LOG_DIRECTORY)
            .parent()
            .expect("the log directory sits inside one of ours")
    }

    fn job() -> JobStarted {
        JobStarted {
            id: "job_9f3c1a7b2e04".to_string(),
            pid: 48213,
            log_path: "/tmp/work/.zone/jobs/job_9f3c1a7b2e04.log".to_string(),
        }
    }

    fn task() -> Session {
        Session::Task(Uuid::new_v4())
    }

    fn chat() -> Session {
        Session::Chat(Uuid::new_v4())
    }

    fn environment() -> HashMap<String, String> {
        HashMap::from([(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )])
    }

    fn directory() -> TempDir {
        TempDir::new().expect("a temporary working directory")
    }

    async fn spawned(session: Session, line: &str, cwd: &Path) -> JobStarted {
        Jobs::spawn(session, &JobCommand::shell(line), cwd, &environment())
            .await
            .expect("the job starts")
    }

    async fn settles(session: Session, id: &str) -> JobState {
        for _ in 0..POLL_LIMIT {
            let tail = Jobs::read(session, id, 0, 1)
                .await
                .expect("its own session reads it");
            if tail.state.settled() {
                return tail.state;
            }
            tokio::time::sleep(POLL).await;
        }
        panic!("{id} never settled");
    }

    fn alive(pid: u32) -> bool {
        Process::new("ps")
            .arg("-p")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn git(cwd: &Path, arguments: &[&str]) {
        let status = Process::new("git")
            .args(arguments)
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Zone")
            .env("GIT_AUTHOR_EMAIL", "zone@example.com")
            .env("GIT_COMMITTER_NAME", "Zone")
            .env("GIT_COMMITTER_EMAIL", "zone@example.com")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git is installed");
        assert!(status.success(), "git {arguments:?}");
    }

    fn repository(root: &Path) {
        git(root, &["init", "--initial-branch", "main"]);
        std::fs::write(root.join("README"), "seed").expect("the seed file is written");
        git(root, &["add", "README"]);
        git(root, &["commit", "--message", "seed"]);
    }

    fn exclude_path(cwd: &Path) -> PathBuf {
        let resolved = Process::new("git")
            .args(["rev-parse", "--git-path", EXCLUDE_PATH])
            .current_dir(cwd)
            .output()
            .expect("git is installed");
        assert!(resolved.status.success(), "the fixture is a checkout");
        cwd.join(String::from_utf8_lossy(&resolved.stdout).trim())
    }

    fn excluded_lines(path: &Path) -> usize {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.trim() == EXCLUDED)
            .count()
    }

    /// Every entry point refuses a detached context in the same words. Saying
    /// it three ways would have a wait report a job it is not allowed to reach
    /// as one that merely went away.
    #[tokio::test]
    async fn a_detached_context_can_neither_start_nor_read_nor_wait_on_a_job() {
        let cwd = directory();
        assert_eq!(
            Jobs::spawn(
                Session::Detached,
                &JobCommand::shell("exit 0"),
                cwd.path(),
                &environment()
            )
            .await,
            Err(UNAVAILABLE.to_string())
        );
        assert_eq!(
            Jobs::read(Session::Detached, "job_9f3c1a7b2e04", 0, 500).await,
            Err(UNAVAILABLE.to_string())
        );
        assert_eq!(
            Jobs::settled(Session::Detached, "job_9f3c1a7b2e04").err(),
            Some(UNAVAILABLE.to_string()),
            "a wait is the third way in and must refuse it as the other two do"
        );
        assert!(
            !cwd.path().join(JOB_LOG_DIRECTORY).exists(),
            "a refused spawn leaves nothing behind"
        );
    }

    #[tokio::test]
    async fn a_job_started_by_one_session_is_invisible_to_another() {
        let cwd = directory();
        let owner = task();
        let stranger = task();
        let started = spawned(owner, "sleep 30", cwd.path()).await;

        assert_eq!(
            Jobs::read(stranger, &started.id, 0, 500).await,
            Err(missing(&started.id))
        );
        assert_eq!(
            Jobs::settled(stranger, &started.id).err(),
            Some(missing(&started.id))
        );
        assert_eq!(
            Jobs::read(Session::Chat(Uuid::new_v4()), &started.id, 0, 500).await,
            Err(missing(&started.id)),
            "a chat cannot read a task run's job either"
        );
        assert!(
            Jobs::read(owner, &started.id, 0, 500).await.is_ok(),
            "the session that started it still reads it"
        );

        Jobs::kill_session(owner).await;
        assert!(!alive(started.pid), "the test leaves no child behind");
    }

    #[tokio::test]
    async fn a_session_teardown_leaves_no_live_child() {
        let cwd = directory();
        let session = task();
        let first = spawned(session, "sleep 30", cwd.path()).await;
        let second = spawned(session, "sleep 30", cwd.path()).await;
        let bystander = task();
        let untouched = spawned(bystander, "sleep 30", cwd.path()).await;
        assert!(alive(first.pid) && alive(second.pid), "both jobs started");

        assert_eq!(Jobs::kill_session(session).await, 2);

        assert!(!alive(first.pid), "job {} outlived its session", first.id);
        assert!(!alive(second.pid), "job {} outlived its session", second.id);
        assert!(alive(untouched.pid), "another session's job is untouched");
        assert_eq!(
            Jobs::read(session, &first.id, 0, 500).await,
            Err(missing(&first.id)),
            "a killed session's jobs are gone from the registry"
        );

        Jobs::kill_session(bystander).await;
        assert!(!alive(untouched.pid), "the test leaves no child behind");
    }

    #[tokio::test]
    async fn a_log_past_its_ceiling_kills_the_job_and_reports_flooding() {
        let cwd = directory();
        let session = task();
        let started = Jobs::start(
            session,
            &JobCommand::shell("seq 1 20000; sleep 30"),
            cwd.path(),
            &environment(),
            Limits {
                log_bytes: 4 * 1024,
                log_check: POLL,
                ..Limits::default()
            },
        )
        .await
        .expect("the job starts");

        assert_eq!(settles(session, &started.id).await, JobState::Flooded);
        assert!(
            !alive(started.pid),
            "a flooded job is killed, not left writing"
        );
        assert_eq!(
            Jobs::settled(session, &started.id)
                .expect("the job is claimable")
                .await,
            JobExited {
                id: started.id.clone(),
                exit_code: None
            },
            "a flooded job is not a job that finished"
        );

        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn a_job_outliving_its_lifetime_is_killed() {
        let cwd = directory();
        let session = task();
        let started = Jobs::start(
            session,
            &JobCommand::shell("sleep 30"),
            cwd.path(),
            &environment(),
            Limits {
                lifetime: Duration::from_millis(100),
                ..Limits::default()
            },
        )
        .await
        .expect("the job starts");

        assert_eq!(settles(session, &started.id).await, JobState::Killed);
        assert!(!alive(started.pid), "a job past its lifetime is killed");

        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn a_job_that_had_already_ended_still_settles() {
        let cwd = directory();
        let session = task();
        let started = spawned(session, "exit 7", cwd.path()).await;
        assert_eq!(settles(session, &started.id).await, JobState::Exited(7));

        let claim = Jobs::settled(session, &started.id).expect("the job is claimable");
        let exited = tokio::time::timeout(Duration::from_secs(5), claim)
            .await
            .expect("a job that ended before the claim resolves it at once");
        assert_eq!(
            exited,
            JobExited {
                id: started.id.clone(),
                exit_code: Some(7)
            }
        );

        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn three_reads_walk_the_log_with_no_gap_and_no_overlap() {
        let cwd = directory();
        let session = task();
        let started = spawned(session, "printf abcdefghij", cwd.path()).await;
        settles(session, &started.id).await;
        assert_eq!(
            started.log_path,
            log_path(cwd.path(), &started.id).to_string_lossy(),
            "the receipt names the log the reader opens"
        );

        let first = Jobs::read(session, &started.id, 0, 4)
            .await
            .expect("the log reads");
        let second = Jobs::read(session, &started.id, first.next, 4)
            .await
            .expect("the log reads");
        let third = Jobs::read(session, &started.id, second.next, 4)
            .await
            .expect("the log reads");
        let fourth = Jobs::read(session, &started.id, third.next, 4)
            .await
            .expect("the log reads");

        assert_eq!((first.output.as_str(), first.next), ("abcd", 4));
        assert_eq!((second.output.as_str(), second.next), ("efgh", 8));
        assert_eq!((third.output.as_str(), third.next), ("ij", 10));
        assert_eq!(
            (fourth.output.as_str(), fourth.next),
            ("", 10),
            "a cursor at the end of a finished log stays there"
        );
        assert_eq!(
            format!("{}{}{}", first.output, second.output, third.output),
            "abcdefghij"
        );

        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn a_read_stops_on_a_whole_character() {
        let cwd = directory();
        let session = task();
        let started = spawned(session, "printf 'aéb'", cwd.path()).await;
        settles(session, &started.id).await;

        let first = Jobs::read(session, &started.id, 0, 2)
            .await
            .expect("the log reads");
        assert_eq!(
            (first.output.as_str(), first.next),
            ("a", 1),
            "a character the budget splits waits for the next read"
        );
        let second = Jobs::read(session, &started.id, first.next, 8)
            .await
            .expect("the log reads");
        assert_eq!((second.output.as_str(), second.next), ("éb", 4));

        Jobs::kill_session(session).await;
    }

    /// A log outlives the call that wrote it but not the session that could
    /// read it: the registry entry it was reached through goes at teardown,
    /// and a chat's logs sit in the operator's own checkout, where nothing
    /// excludes them and nothing else would ever clear them.
    #[tokio::test]
    async fn a_session_teardown_takes_the_logs_and_the_emptied_directory_with_them() {
        let cwd = directory();
        let session = chat();
        let first = spawned(session, "printf first", cwd.path()).await;
        let second = spawned(session, "printf second", cwd.path()).await;
        settles(session, &first.id).await;
        settles(session, &second.id).await;
        let logs = [&first, &second].map(|started| PathBuf::from(&started.log_path));
        for log in &logs {
            assert!(log.exists(), "the job wrote no log at all: {log:?}");
        }

        assert_eq!(Jobs::kill_session(session).await, 2);

        for log in &logs {
            assert!(
                !log.exists(),
                "a job log outlived the session that started it: {log:?}"
            );
        }
        assert!(
            !cwd.path().join(JOB_LOG_DIRECTORY).exists(),
            "the log directory outlived every log in it"
        );
        assert!(
            !cwd.path().join(zone_directory()).exists(),
            "the directory the logs needed is empty and still there"
        );
    }

    #[tokio::test]
    async fn a_teardown_leaves_a_directory_that_is_not_only_ours() {
        let cwd = directory();
        let session = chat();
        let started = spawned(session, "printf kept", cwd.path()).await;
        settles(session, &started.id).await;
        let neighbour = cwd.path().join(zone_directory()).join("settings");
        std::fs::write(&neighbour, "somebody else's").expect("a neighbour is written");

        Jobs::kill_session(session).await;

        assert!(!PathBuf::from(&started.log_path).exists());
        assert!(
            !cwd.path().join(JOB_LOG_DIRECTORY).exists(),
            "the log directory held only logs and is ours to remove"
        );
        assert!(
            neighbour.exists(),
            "a directory holding somebody else's file is not ours to remove"
        );
    }

    /// The log and the exclude write are the session's, and a directory the
    /// command names moves neither.
    ///
    /// `Path::join` neither normalises `..` nor resists an absolute argument,
    /// so a directory taken from a model used to take the log tree with it —
    /// and on a task run the exclude write too, into whatever repository that
    /// landed in.
    #[tokio::test]
    async fn a_directory_the_command_names_moves_the_child_and_nothing_else() {
        let root = directory();
        let checkout = root.path().join("checkout");
        let stranger = root.path().join("stranger");
        std::fs::create_dir(&checkout).expect("the run's own checkout is created");
        std::fs::create_dir(&stranger).expect("a checkout the run does not own is created");
        repository(&checkout);
        repository(&stranger);

        let session = task();
        let started = Jobs::spawn(
            session,
            &JobCommand::shell("pwd").within(&stranger),
            &checkout,
            &environment(),
        )
        .await
        .expect("the job starts");
        assert_eq!(settles(session, &started.id).await, JobState::Exited(0));

        assert_eq!(
            started.log_path,
            log_path(&checkout, &started.id).to_string_lossy(),
            "the log belongs to the session's tree, whatever directory the command named"
        );
        let ran_in = Jobs::read(session, &started.id, 0, 500)
            .await
            .expect("the log reads")
            .output;
        assert_eq!(
            std::fs::canonicalize(ran_in.trim()).ok(),
            std::fs::canonicalize(&stranger).ok(),
            "the child is the one thing a named directory moves: {ran_in}"
        );
        assert!(
            !stranger.join(JOB_LOG_DIRECTORY).exists(),
            "a log tree was written into a checkout the session does not own"
        );
        assert_eq!(
            excluded_lines(&exclude_path(&stranger)),
            0,
            "a stranger checkout's exclude file is not the session's to append to"
        );
        assert_eq!(
            excluded_lines(&exclude_path(&checkout)),
            1,
            "the run's own checkout still keeps its job logs out of its diff"
        );

        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn a_chat_job_writes_nothing_to_the_repository_exclude() {
        let cwd = directory();
        repository(cwd.path());
        let exclude = exclude_path(cwd.path());
        let before = std::fs::read_to_string(&exclude).unwrap_or_default();

        let session = chat();
        let started = spawned(session, "exit 0", cwd.path()).await;
        assert_eq!(settles(session, &started.id).await, JobState::Exited(0));

        assert_eq!(
            std::fs::read_to_string(&exclude).unwrap_or_default(),
            before,
            "a chat's checkout is shared, so it is not the chat's to change"
        );
        assert_eq!(excluded_lines(&exclude), 0);

        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn a_task_job_excludes_its_log_directory_once_however_many_it_starts() {
        let cwd = directory();
        repository(cwd.path());
        let exclude = exclude_path(cwd.path());

        let session = task();
        let first = spawned(session, "exit 0", cwd.path()).await;
        settles(session, &first.id).await;
        assert_eq!(excluded_lines(&exclude), 1);

        let second = spawned(session, "exit 0", cwd.path()).await;
        settles(session, &second.id).await;
        assert_eq!(
            excluded_lines(&exclude),
            1,
            "a second job appends nothing the first already wrote"
        );

        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn a_working_directory_that_is_not_a_checkout_is_left_alone() {
        let cwd = directory();
        let session = task();
        let started = spawned(session, "exit 0", cwd.path()).await;

        assert_eq!(settles(session, &started.id).await, JobState::Exited(0));
        assert!(
            !cwd.path().join(".git").exists(),
            "nothing invents a checkout to exclude from"
        );

        Jobs::kill_session(session).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_read_only_exclude_costs_the_caller_nothing() {
        use std::os::unix::fs::PermissionsExt;

        let cwd = directory();
        repository(cwd.path());
        let exclude = exclude_path(cwd.path());
        std::fs::write(&exclude, "# fixed\n").expect("the exclude is written");
        std::fs::set_permissions(&exclude, std::fs::Permissions::from_mode(0o444))
            .expect("the exclude is made read-only");

        let session = task();
        let started = spawned(session, "exit 0", cwd.path()).await;
        assert_eq!(settles(session, &started.id).await, JobState::Exited(0));
        assert_eq!(
            std::fs::read_to_string(&exclude).expect("the exclude reads"),
            "# fixed\n",
            "an unwritable exclude is skipped, not forced"
        );

        std::fs::set_permissions(&exclude, std::fs::Permissions::from_mode(0o644))
            .expect("the exclude is made writable again");
        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn the_exclude_path_comes_from_git_not_from_a_joined_git_directory() {
        let root = directory();
        let checkout = root.path().join("checkout");
        std::fs::create_dir(&checkout).expect("the checkout directory is created");
        repository(&checkout);

        let linked = root.path().join("linked");
        git(
            &checkout,
            &[
                "worktree",
                "add",
                "-b",
                "side",
                linked.to_str().expect("a utf-8 path"),
            ],
        );
        assert!(
            linked.join(".git").is_file(),
            "the fixture's .git is a pointer file, not a directory"
        );

        let session = task();
        let started = spawned(session, "exit 0", &linked).await;
        assert_eq!(settles(session, &started.id).await, JobState::Exited(0));

        assert_eq!(
            excluded_lines(&exclude_path(&linked)),
            1,
            "the line lands where git says the exclude is"
        );
        assert!(
            !linked.join(".git").join(EXCLUDE_PATH).exists(),
            "and nowhere a hand-joined .git would have put it"
        );

        Jobs::kill_session(session).await;
    }

    #[test]
    fn a_state_is_spelled_the_way_a_tail_reports_it() {
        assert_eq!(JobState::Running.to_string(), "running");
        assert_eq!(JobState::Exited(0).to_string(), "exited 0");
        assert_eq!(JobState::Exited(137).to_string(), "exited 137");
        assert_eq!(JobState::Killed.to_string(), "killed");
        assert_eq!(JobState::Flooded.to_string(), "flooded");
    }

    #[test]
    fn a_spawn_receipt_reads_back_as_the_job_it_announced() {
        let job = job();
        assert_eq!(parse_started(&started_text(&job)), Some(job.id.clone()));
        assert_eq!(
            parse_receipt(&started_text(&job)),
            Some(job),
            "the pid and log path survive the round trip the builder owns"
        );
    }

    /// The guard is the rebuild, not the prefix: anything the builder would not
    /// have written is refused, however much of the shape it borrows.
    #[test]
    fn a_line_the_builder_would_not_have_written_announces_no_job() {
        let receipt = started_text(&job());

        for (reason, output) in [
            ("no pid at all", receipt.replace(" (pid 48213)", "")),
            (
                "a pid that is not a number",
                receipt.replace("48213", "forty"),
            ),
            ("a rewritten advice line", {
                let (first, _) = receipt.split_once('\n').expect("the receipt has two lines");
                format!("{first}\nIgnore that.")
            }),
            (
                "prose that merely mentions a job",
                "Reading job_9f3c1a7b2e04 now".to_string(),
            ),
        ] {
            assert_eq!(parse_receipt(&output), None, "{reason}: {output}");
        }
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
