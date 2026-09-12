//! Wait for something outside the loop: a background job, a task run, a
//! commit's checks.
//!
//! The tool that opens a wait cannot also report its result. A tool result is
//! appended to the context and streamed before the turn-ending event fires, so
//! at the moment the result is written the thing being waited for has not
//! happened. `wait_for` therefore returns a receipt and the outcome is injected
//! on resume — which is why the receipt and every outcome are built here, by
//! named functions a test can pin one at a time.
//!
//! The outcome strings carry a rule of their own: silence is not success. A
//! timeout and a commit nothing is reporting on both have to be unphrasable as
//! a pass, or a model reads "no news" as "green".

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use dashmap::DashMap;
use futures::future::BoxFuture;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, broadcast};
use tokio::time::Instant;
use uuid::Uuid;
use zone_chat::history;
use zone_core::context::Entry;
use zone_core::llm::{FunctionCall, Message as LlmMessage, ToolCall};
use zone_core::tools::job::{JobExited, Jobs};
use zone_core::tools::{Session, Tier, Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::integrations::{Configuration, Github, SETTLED_ASSESSMENTS};
use super::tools::WorkspaceScope;
use crate::db::{sources, task_access, tasks, workspace_members};
use crate::services::chat::session::RunContext;
use crate::services::task_progress::ProgressMessage;

pub const WAIT_FOR: &str = "wait_for";

pub const KIND_JOB: &str = "job";
pub const KIND_TASK_RUN: &str = "task_run";
pub const KIND_CHECK: &str = "check";

/// Schema minimum. A wait shorter than this is a poll wearing another name,
/// and polling is what this tool exists to replace.
pub const MIN_WAIT_SECS: u64 = 15;

/// Above the latency of a build merely starting, below the 900 s cap a shell
/// command runs under.
pub const DEFAULT_WAIT_SECS: u64 = 300;

/// Half the task timeout. On chat it is the whole turn, so the clamp there is
/// against whatever remains of the stream deadline.
pub const MAX_WAIT_SECS: u64 = 1_800;

/// The default has to sit inside the window the schema advertises, or the tool
/// would offer a bound it rejects.
const _: () = assert!(MIN_WAIT_SECS < DEFAULT_WAIT_SECS && DEFAULT_WAIT_SECS < MAX_WAIT_SECS);

/// A refunded iteration still spends a tool call, and `max_tool_calls` is the
/// only bound that existed before this tool. This is what actually caps how
/// often one attempt can park and re-queue for its admission slot.
pub const MAX_WAITS_PER_ATTEMPT: usize = 10;

/// Three fully-paginated GitHub reads per poll: 120 polls an hour is about 7%
/// of the authenticated hourly budget, so a dozen concurrent waits still fit.
pub const CHECK_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// How long a commit that nothing is reporting on is tolerated before the wait
/// ends. An empty check list reads as unknown, never as pending, so without a
/// grace period a wait opened straight after a push would settle on nothing.
pub const CHECK_SETTLE_GRACE: Duration = Duration::from_secs(120);

/// Which way a wait ended.
///
/// The outcome beside it is prose written for the model, and prose is not a
/// contract: the console once decided what to draw by reading the opening
/// words, and the one outcome that says out loud "this is not a pass" began
/// with the same three words as an ordinary settle, so it was drawn as a
/// finished wait. Every outcome is built with its verdict here instead, and the
/// card reads nothing but this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The subject reached an end of its own: a job exited or was killed, a run
    /// finished, a commit's checks concluded pass or fail.
    Settled,
    /// The window ran out with the subject still going, or with nothing left
    /// holding the wait open.
    TimedOut,
    /// The grace period elapsed with nothing reporting on the commit.
    Silent,
    /// The grace period elapsed with the commit's checks unreadable.
    Unreadable,
}

/// An outcome as both the things it has to be: prose the model reads, and the
/// verdict the console draws on. Built together so neither can be derived from
/// the other after the fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub verdict: Verdict,
    pub text: String,
}

impl Outcome {
    fn settled(text: String) -> Self {
        Self {
            verdict: Verdict::Settled,
            text,
        }
    }

    fn timed_out(text: String) -> Self {
        Self {
            verdict: Verdict::TimedOut,
            text,
        }
    }

    fn silent(text: String) -> Self {
        Self {
            verdict: Verdict::Silent,
            text,
        }
    }

    fn unreadable(text: String) -> Self {
        Self {
            verdict: Verdict::Unreadable,
            text,
        }
    }
}

/// What one registered wait is waiting for, for the console's card and for the
/// loop's own turn-ending event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waiting {
    pub kind: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// RFC 3339. Already clamped by `wait_for::execute`.
    pub deadline: String,
}

/// How a wait ended, for the console. The model is told the same thing through
/// the injected outcome entry instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitSettled {
    pub tool_call_id: String,
    pub outcome: String,
    pub verdict: Verdict,
}

/// What the subject of a wait is called, mid-sentence.
pub fn job_subject(id: &str) -> String {
    id.to_string()
}

pub fn task_run_subject(run: Uuid) -> String {
    format!("task run {run}")
}

pub fn check_subject(reference: &str, sha: &str) -> String {
    format!("checks on {reference} ({})", short(sha))
}

/// What `wait_for` returns the moment it has registered the wait.
pub fn receipt(subject: &str, deadline: &str) -> String {
    format!("Waiting for {subject} until {deadline}.")
}

pub fn job_exited(id: &str, exit_code: i32, elapsed: Duration) -> Outcome {
    Outcome::settled(format!(
        "{id} exited with code {exit_code} after {}s.",
        elapsed.as_secs()
    ))
}

pub fn job_killed(id: &str, elapsed: Duration) -> Outcome {
    Outcome::settled(format!(
        "{id} was killed after {}s without exiting.",
        elapsed.as_secs()
    ))
}

pub fn task_run_completed(run: Uuid, elapsed: Duration) -> Outcome {
    Outcome::settled(format!(
        "Task run {run} completed after {}s.",
        elapsed.as_secs()
    ))
}

pub fn task_run_failed(run: Uuid, elapsed: Duration, error: &str) -> Outcome {
    Outcome::settled(format!(
        "Task run {run} failed after {}s: {error}",
        elapsed.as_secs()
    ))
}

pub fn checks_settled(reference: &str, sha: &str, assessment: &str, elapsed: Duration) -> Outcome {
    Outcome::settled(format!(
        "Checks on {reference} ({}) settled to {assessment} after {}s.",
        short(sha),
        elapsed.as_secs()
    ))
}

/// How [`checks_unknown`] opens.
pub const CHECKS_UNKNOWN_PREFIX: &str = "No checks are configured or reporting on";

/// A commit whose checks could not be read, after [`CHECK_SETTLE_GRACE`].
///
/// An outage is not an empty check list. Reporting one as the other blames the
/// repository for GitHub being unreachable and hides the only thing that would
/// tell a model to try again, so the two are separate strings — both of which
/// have to be unphrasable as a pass.
pub fn checks_unreadable(reference: &str, reason: &str, elapsed: Duration) -> Outcome {
    Outcome::unreadable(format!(
        "The checks on {reference} could not be read after {}s, so this is not a pass: {reason}",
        elapsed.as_secs()
    ))
}

/// A commit nothing is reporting on, after [`CHECK_SETTLE_GRACE`]. Deliberately
/// says out loud that it is not a pass.
pub fn checks_unknown(reference: &str, elapsed: Duration) -> Outcome {
    Outcome::silent(format!(
        "{CHECKS_UNKNOWN_PREFIX} {reference} after {}s. This is not a pass.",
        elapsed.as_secs()
    ))
}

/// The wait ran out. Says what did *not* happen, because a model handed a bare
/// "finished" would act as though it had.
pub fn timed_out(subject: &str, waited: Duration) -> Outcome {
    Outcome::timed_out(format!(
        "Timed out after {}s. {subject} has not finished — this is a timeout, not a result. \
         Check again or wait longer.",
        waited.as_secs()
    ))
}

/// Returned as a tool error, so the turn never ends and nothing parks.
pub fn too_many_waits() -> String {
    format!(
        "You have waited {MAX_WAITS_PER_ATTEMPT} times in this attempt, which is the limit. \
         Act on what you already have."
    )
}

/// Returned as a tool error for the same reason as [`too_many_waits`]: a
/// window the surface cannot hold would be acknowledged with a receipt, time
/// out the moment it was awaited, and spend one of the allowances on nothing.
pub fn no_window_left() -> String {
    format!(
        "Less than {MIN_WAIT_SECS}s of this turn remains, which is too little to wait in. \
         Act on what you already have."
    )
}

pub fn task_run_wait_unavailable() -> String {
    format!(
        "Waiting on another task run is not available from a task run. Use kind={KIND_JOB} or \
         kind={KIND_CHECK}, or finish and let whoever started this run coordinate."
    )
}

/// How much of a commit a subject line quotes: enough to recognise, short
/// enough to read.
const SHA_CHARS: usize = 7;

fn short(sha: &str) -> &str {
    sha.get(..SHA_CHARS).unwrap_or(sha)
}

/// What `wait_for` tells the model it is for.
const DESCRIPTION: &str = "Wait for something outside this loop to finish: a background job, \
    another task run, or the checks on a commit. Your turn ends the moment you call this and \
    resumes with the outcome, so call it alone, size the timeout to what you are waiting for, \
    and do not use it to re-read something that has not changed.";

/// The two statuses a run that has not finished can hold, and the one that says
/// it finished well.
const RUN_RUNNING: &str = "running";
const RUN_WAITING: &str = "waiting";
const RUN_COMPLETED: &str = "completed";

/// What a failed run is called when it recorded no message of its own.
const RUN_UNKNOWN_ERROR: &str = "Unknown error";

/// A commit GitHub has registered no run for at all. Never a settle on its own:
/// right after a push it is indistinguishable from a repository with no CI.
const UNKNOWN_ASSESSMENT: &str = "unknown";

const FUNCTION_CALL: &str = "function";

/// The suffix that tells the injected envelope apart from the call that opened
/// the wait. The receipt already claimed the original id, and both the context's
/// uniqueness check and the store's duplicate-result check reject a second use.
const SETTLED_SUFFIX: &str = "#settled";

const INVALID_ARGUMENTS: &str = "wait_for takes kind, id, an optional ref and an optional \
    timeout_secs.";

/// A tool set with no workspace can still wait on its own jobs, but a run and a
/// source are a workspace's to read.
const WORKSPACE_REQUIRED: &str =
    "Waiting on a task run or a check is not available in this context.";

const RUN_ID_INVALID: &str = "id must be a task run id for kind=task_run.";
const SOURCE_ID_INVALID: &str = "id must be a source id for kind=check.";
const RUN_UNREADABLE: &str = "Could not read the task run.";
const RUN_FOREIGN: &str = "Task run not found in this workspace.";
const SOURCE_UNREADABLE: &str = "Could not read the source.";
const SOURCE_FOREIGN: &str = "Source not found in this workspace or inactive.";
const SOURCE_NOT_GITHUB: &str = "Waiting on checks currently supports connected GitHub sources \
    only.";
const SOURCE_CONFIGURATION_INVALID: &str = "The GitHub source configuration is invalid.";
const SOURCE_CREDENTIALS_INVALID: &str = "The source credentials could not be decrypted.";
const WORKSPACE_UNREADABLE: &str = "You cannot read this workspace.";

/// What a wait nobody registered is called, so even that reads as a timeout
/// rather than as anything having happened.
const LOST_SUBJECT: &str = "This wait";

fn unknown_kind(kind: &str) -> String {
    format!("kind must be {KIND_JOB}, {KIND_TASK_RUN} or {KIND_CHECK}, not {kind:?}.")
}

/// The call that waited, and what it waited for.
///
/// The pair travels together from the turn-ending event to the injection that
/// resumes the run, because the outcome entry is minted from both.
#[derive(Debug, Clone, PartialEq)]
pub struct Waited {
    pub tool_call_id: String,
    pub waiting: Waiting,
}

impl Waited {
    pub fn new(tool_call_id: impl Into<String>, waiting: Waiting) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            waiting,
        }
    }
}

/// What one poll of a commit's checks reports, or why it could not be read.
///
/// A trait rather than the client directly so the grace period and the settle
/// rule can be proven without a network, which the client's fixed origin
/// otherwise requires.
#[async_trait]
trait Checks: Send + Sync {
    async fn assess(&self, sha: &str) -> Result<&'static str, String>;
}

#[async_trait]
impl Checks for Github {
    /// Polled against the resolved SHA rather than the reference, so a branch
    /// that moves mid-wait does not silently change what is being waited on.
    ///
    /// The failure is carried rather than dropped: a poll GitHub refused says
    /// nothing about what the checks are, and reading it as "no answer" is
    /// what let an outage end the wait as a commit nothing reports on.
    async fn assess(&self, sha: &str) -> Result<&'static str, String> {
        self.settled(Some(sha))
            .await
            .map(|(_, _, assessment)| assessment)
    }
}

/// A task run that already reached a terminal status.
struct Finished {
    completed: bool,
    error: Option<String>,
}

impl Finished {
    fn read(row: &tasks::TaskRunRow) -> Option<Self> {
        if matches!(row.status.as_str(), RUN_RUNNING | RUN_WAITING) {
            return None;
        }
        Some(Self {
            completed: row.status == RUN_COMPLETED,
            error: row.error_message.clone(),
        })
    }

    fn outcome(&self, run: Uuid, elapsed: Duration) -> Outcome {
        if self.completed {
            task_run_completed(run, elapsed)
        } else {
            task_run_failed(
                run,
                elapsed,
                self.error.as_deref().unwrap_or(RUN_UNKNOWN_ERROR),
            )
        }
    }
}

struct Run {
    run: Uuid,
    events: broadcast::Receiver<ProgressMessage>,
    finished: Option<Finished>,
    database: PgPool,
}

impl Run {
    async fn settle(mut self, started: Instant) -> Outcome {
        if let Some(finished) = self.finished {
            return finished.outcome(self.run, started.elapsed());
        }
        loop {
            match self.events.recv().await {
                Ok(ProgressMessage::Completed { .. }) => {
                    return task_run_completed(self.run, started.elapsed());
                }
                Ok(ProgressMessage::Failed { error }) => {
                    return task_run_failed(self.run, started.elapsed(), &error);
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    // The terminal writer drops the channel right after it
                    // publishes, so a closed channel means the run ended and
                    // the row is the only place left to read the ending from.
                    return match self.terminal().await {
                        Some(finished) => finished.outcome(self.run, started.elapsed()),
                        None => std::future::pending().await,
                    };
                }
            }
        }
    }

    async fn terminal(&self) -> Option<Finished> {
        let row = tasks::get_task_run(&self.database, self.run).await.ok()??;
        Finished::read(&row)
    }
}

struct Commit {
    checks: Box<dyn Checks>,
    reference: String,
    sha: String,
    assessment: &'static str,
}

impl Commit {
    async fn settle(mut self, started: Instant) -> Outcome {
        let mut unknown_since = None;
        let mut unreadable: Option<String> = None;
        loop {
            if SETTLED_ASSESSMENTS.contains(&self.assessment) {
                return checks_settled(
                    &self.reference,
                    &self.sha,
                    self.assessment,
                    started.elapsed(),
                );
            }
            if self.assessment == UNKNOWN_ASSESSMENT {
                let since = *unknown_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= CHECK_SETTLE_GRACE {
                    return match unreadable {
                        Some(reason) => {
                            checks_unreadable(&self.reference, &reason, started.elapsed())
                        }
                        None => checks_unknown(&self.reference, started.elapsed()),
                    };
                }
            } else {
                unknown_since = None;
            }
            tokio::time::sleep(CHECK_POLL_INTERVAL).await;
            match self.checks.assess(&self.sha).await {
                Ok(assessment) => {
                    self.assessment = assessment;
                    unreadable = None;
                }
                Err(reason) => unreadable = Some(reason),
            }
        }
    }
}

/// The already-open handle a registered wait sits on.
enum Subscription {
    Job(BoxFuture<'static, JobExited>),
    Run(Run),
    Commit(Commit),
    /// Nothing to observe, so the deadline is the only way this ends.
    Deadline,
}

impl Subscription {
    async fn settle(self, started: Instant) -> Outcome {
        match self {
            Self::Deadline => std::future::pending().await,
            Self::Job(exit) => {
                let JobExited { id, exit_code } = exit.await;
                match exit_code {
                    Some(code) => job_exited(&id, code, started.elapsed()),
                    None => job_killed(&id, started.elapsed()),
                }
            }
            Self::Run(run) => run.settle(started).await,
            Self::Commit(commit) => commit.settle(started).await,
        }
    }
}

/// One open wait: what it is for, what it sits on, and when it started.
///
/// The clock starts at registration rather than at the await, so the elapsed
/// time an outcome reports is how long since the model asked.
struct Registration {
    waiting: Waiting,
    subject: String,
    started: Instant,
    subscription: Mutex<Subscription>,
}

/// What one open wait is filed under.
///
/// A tool-call id is unique inside a session and nothing makes it unique
/// across them — a local model mints `call_1` for the first call of every turn
/// — so the session is half the identity. Keyed by the id alone, one session's
/// registration replaced another's and each then settled with the other's
/// subject.
type Key = (Session, String);

fn key(session: Session, tool_call_id: &str) -> Key {
    (session, tool_call_id.to_string())
}

/// Every open wait on this instance, keyed by the session and the tool call
/// that opened it.
///
/// Process-local and not persisted, the same single-instance assumption the run
/// socket and the question waiter already document: what is being waited on is
/// a child process or a subscription this process holds, so a surviving row
/// would claim a durability the thing does not have.
static REGISTERED: Lazy<DashMap<Key, Registration>> = Lazy::new(DashMap::new);

/// Waits opened but not yet tied to the call that opened them.
///
/// `Tool::execute` is never told the id of the call it is serving, so the tool
/// claims its wait under the only identity it has. [`bind`] then binds it to
/// the call id. One session can hold exactly one unclaimed wait: an
/// ends-turn tool is never batched with anything and the loop returns the
/// moment one completes.
static PENDING: Lazy<DashMap<Session, Registration>> = Lazy::new(DashMap::new);

/// How many waits a session has opened, and how far out any of them may reach.
#[derive(Default)]
struct Allowance {
    taken: usize,
    ceiling: Option<Instant>,
}

static SESSIONS: Lazy<DashMap<Session, Allowance>> = Lazy::new(DashMap::new);

/// Claim the wait before the receipt is written.
///
/// The receipt is appended and streamed before the turn-ending event fires, so
/// a wait registered any later could be settled by something that happened
/// while the receipt was still in flight, with nobody subscribed to notice.
fn stage(session: Session, registration: Registration) {
    PENDING.insert(session, registration);
}

/// Register an open wait under the call that opened it.
///
/// The direct form, for a caller holding both the call id and what is being
/// waited for. `wait_for::execute` cannot use it: `Tool::execute` is never told
/// which call it is serving, so it claims by session and the loop binds the two
/// together. A wait registered here has nothing subscribed behind it and ends
/// at its own deadline.
pub fn claim(tool_call_id: &str, waiting: Waiting, session: Session) {
    REGISTERED.insert(
        key(session, tool_call_id),
        Registration {
            subject: subject(&waiting),
            waiting,
            started: Instant::now(),
            subscription: Mutex::new(Subscription::Deadline),
        },
    );
}

/// What a wait is called mid-sentence, from the wait alone.
///
/// A commit's subject quotes the SHA its reference resolved to, which only the
/// call that resolved it holds, so that one is built there instead.
fn subject(waiting: &Waiting) -> String {
    match waiting.kind.as_str() {
        KIND_TASK_RUN => {
            Uuid::parse_str(&waiting.id).map_or_else(|_| waiting.id.clone(), task_run_subject)
        }
        KIND_CHECK => format!(
            "checks on {}",
            waiting.reference.as_deref().unwrap_or(&waiting.id)
        ),
        _ => job_subject(&waiting.id),
    }
}

/// Tie a session's unclaimed wait to the call that opened it, and say what it
/// is waiting for.
///
/// The loop reads the clamped deadline from the answer rather than re-parsing
/// the call's arguments, because the clamp is `wait_for::execute`'s to apply.
pub fn bind(session: Session, tool_call_id: &str) -> Option<Waiting> {
    let (_, registration) = PENDING.remove(&session)?;
    let waiting = registration.waiting.clone();
    REGISTERED.insert(key(session, tool_call_id), registration);
    Some(waiting)
}

/// How many waits this session has already opened.
pub fn waits_taken(session: Session) -> usize {
    SESSIONS.get(&session).map_or(0, |entry| entry.taken)
}

/// Hold this session's waits inside a deadline the surface owns.
///
/// Chat's whole turn is bounded by its stream deadline, which is computed after
/// the tool set exists, so it reaches the tool here rather than through the
/// context the tool set already froze. Tasks set none: the attempt timeout
/// already covers every wait inside it.
pub fn set_ceiling(session: Session, ceiling: Instant) {
    SESSIONS.entry(session).or_default().ceiling = Some(ceiling);
}

fn ceiling(session: Session) -> Option<Instant> {
    SESSIONS.get(&session).and_then(|entry| entry.ceiling)
}

/// Forget what a session spent and how far it could reach.
///
/// Called once per chat turn and at the top of each pass of a task's attempt
/// loop, so a retried attempt does not inherit a spent counter. Unclaimed waits
/// go with it: a turn cancelled between the receipt and the park would
/// otherwise leave its subscription open for the life of the process.
pub fn reset_session(session: Session) {
    SESSIONS.remove(&session);
    PENDING.remove(&session);
    REGISTERED.retain(|(owner, _), _| *owner != session);
}

/// How long a wait may run, clamped to what the schema advertises and then to
/// whatever the surface still has left.
///
/// Nothing when what the surface has left is under the floor: clamping to it
/// would mint a window that expires the moment it is awaited, and a wait
/// shorter than [`MIN_WAIT_SECS`] is what this tool exists to replace.
fn window(requested: Option<u64>, session: Session) -> Option<Duration> {
    let seconds = requested
        .unwrap_or(DEFAULT_WAIT_SECS)
        .clamp(MIN_WAIT_SECS, MAX_WAIT_SECS);
    let window = Duration::from_secs(seconds);
    let Some(ceiling) = ceiling(session) else {
        return Some(window);
    };
    let remaining = ceiling.saturating_duration_since(Instant::now());
    (remaining >= Duration::from_secs(MIN_WAIT_SECS)).then(|| window.min(remaining))
}

/// When a registered wait runs out, as a deadline this process can sleep to.
pub fn deadline(waiting: &Waiting) -> Instant {
    let remaining = DateTime::parse_from_rfc3339(&waiting.deadline)
        .ok()
        .and_then(|deadline| (deadline.with_timezone(&Utc) - Utc::now()).to_std().ok())
        .unwrap_or_default();
    Instant::now() + remaining
}

/// Wait for what `tool_call_id` registered, or for its deadline.
///
/// Polled in order, not at random: something that settles in the same tick as
/// the deadline elapses has settled, and reporting that as a timeout would tell
/// the model nothing had happened when it had.
pub async fn await_outcome(session: Session, tool_call_id: &str, deadline: Instant) -> Outcome {
    let Some((_, registration)) = REGISTERED.remove(&key(session, tool_call_id)) else {
        return timed_out(LOST_SUBJECT, Duration::ZERO);
    };
    let started = registration.started;
    let subject = registration.subject;
    let subscription = registration.subscription.into_inner();
    tokio::select! {
        biased;
        outcome = subscription.settle(started) => outcome,
        _ = tokio::time::sleep_until(deadline) => timed_out(&subject, started.elapsed()),
    }
}

fn settled_call_id(tool_call_id: &str) -> String {
    format!("{tool_call_id}{SETTLED_SUFFIX}")
}

/// Add the outcome as the newest preserved evidence, demoting the outcomes
/// earlier waits added and nothing else.
///
/// It arrives as an envelope and its result rather than as a bare message
/// because those are the only shapes a turn accepts: a chat store takes no
/// other role, the call that opened the wait has already claimed its one
/// result, and compaction groups an envelope with the results that immediately
/// follow it. Returns the pair, for a caller with somewhere durable to put it.
pub fn resume_with_outcome(
    context: &mut RunContext,
    waited: &Waited,
    outcome: String,
    waits: &mut Vec<String>,
) -> [history::NewEntry; 2] {
    for entry in &mut context.entries {
        if waits.contains(&entry.id) {
            entry.preserve = false;
        }
    }
    let call = settled_call_id(&waited.tool_call_id);
    let envelope = LlmMessage::assistant_with_tools(vec![ToolCall {
        id: call.clone(),
        call_type: FUNCTION_CALL.to_string(),
        function: FunctionCall {
            name: WAIT_FOR.to_string(),
            arguments: serde_json::to_string(&waited.waiting)
                .unwrap_or_else(|_| json!({}).to_string()),
        },
    }]);
    let result = LlmMessage::tool_result(call, outcome);
    let pair = [envelope, result].map(|message| history::NewEntry {
        id: Uuid::new_v4().to_string(),
        message: (&message).into(),
        mutations: Vec::new(),
    });
    for entry in &pair {
        context.entries.push(Entry {
            id: entry.id.clone(),
            message: entry.message.clone().into_message(),
            preserve: true,
            consumed: false,
        });
        waits.push(entry.id.clone());
    }
    pair
}

pub fn register(registry: &mut ToolRegistry, scope: Option<&WorkspaceScope>) {
    registry.register(Arc::new(WaitForTool {
        scope: scope.cloned(),
    }));
}

#[derive(Debug, Deserialize)]
struct Request {
    kind: String,
    id: String,
    #[serde(rename = "ref")]
    reference: Option<String>,
    timeout_secs: Option<u64>,
}

struct WaitForTool {
    scope: Option<WorkspaceScope>,
}

#[async_trait]
impl Tool for WaitForTool {
    fn name(&self) -> &str {
        WAIT_FOR
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": [KIND_JOB, KIND_TASK_RUN, KIND_CHECK]},
                "id": {
                    "type": "string",
                    "description": format!(
                        "Job id for kind={KIND_JOB}, run id for kind={KIND_TASK_RUN}, \
                         source id for kind={KIND_CHECK}."
                    )
                },
                "ref": {
                    "type": "string",
                    "description": format!(
                        "kind={KIND_CHECK} only: branch, tag or commit. Defaults to the source \
                         branch."
                    )
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": MIN_WAIT_SECS,
                    "maximum": MAX_WAIT_SECS,
                    "description": format!(
                        "How long to wait. Default {DEFAULT_WAIT_SECS}. Size it to what you are \
                         waiting for."
                    )
                }
            },
            "required": ["kind", "id"],
            "additionalProperties": false
        })
    }

    fn tier(&self) -> Tier {
        Tier::Read
    }

    fn ends_turn(&self) -> bool {
        true
    }

    /// Authorize, resolve and register — never await.
    ///
    /// The receipt is appended and streamed strictly before the turn-ending
    /// event fires, so at the moment it is written the thing being waited for
    /// has not happened. The outcome is injected on resume instead. That is
    /// also why the trait's own timeout is left alone: nothing here waits.
    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(match self.open(params, context).await {
            Ok(receipt) => ToolResult::success(receipt),
            Err(refusal) => ToolResult::error(refusal),
        })
    }
}

impl WaitForTool {
    async fn open(&self, params: Value, context: &ToolContext) -> Result<String, String> {
        let request: Request =
            serde_json::from_value(params).map_err(|_| INVALID_ARGUMENTS.to_string())?;
        if waits_taken(context.session) >= MAX_WAITS_PER_ATTEMPT {
            return Err(too_many_waits());
        }
        let Some(window) = window(request.timeout_secs, context.session) else {
            return Err(no_window_left());
        };
        let started = Instant::now();

        let (subject, reference, subscription) = match request.kind.as_str() {
            KIND_JOB => self.job(&request, context)?,
            KIND_TASK_RUN => self.run(&request, context).await?,
            KIND_CHECK => self.commit(&request).await?,
            other => return Err(unknown_kind(other)),
        };

        let waiting = Waiting {
            kind: request.kind,
            id: request.id,
            reference,
            deadline: (Utc::now() + window).to_rfc3339_opts(SecondsFormat::Secs, true),
        };
        let receipt = receipt(&subject, &waiting.deadline);
        stage(
            context.session,
            Registration {
                waiting,
                subject,
                started,
                subscription: Mutex::new(subscription),
            },
        );
        SESSIONS.entry(context.session).or_default().taken += 1;
        Ok(receipt)
    }

    fn job(
        &self,
        request: &Request,
        context: &ToolContext,
    ) -> Result<(String, Option<String>, Subscription), String> {
        let exit = Jobs::settled(context.session, &request.id)?;
        Ok((
            job_subject(&request.id),
            None,
            Subscription::Job(Box::pin(exit)),
        ))
    }

    /// A run is readable to every member of the workspace that owns it, which
    /// is not necessarily the caller's own, so membership is proven and then
    /// the run's workspace is checked against the one this tool set speaks for.
    async fn run(
        &self,
        request: &Request,
        context: &ToolContext,
    ) -> Result<(String, Option<String>, Subscription), String> {
        if matches!(context.session, Session::Task(_)) {
            return Err(task_run_wait_unavailable());
        }
        let scope = self.scope.as_ref().ok_or(WORKSPACE_REQUIRED)?;
        let run = Uuid::parse_str(&request.id).map_err(|_| RUN_ID_INVALID.to_string())?;
        let database = scope.state.db();
        let snapshot = task_access::read(database, run, scope.user_id)
            .await
            .map_err(|_| RUN_UNREADABLE.to_string())?
            .ok_or(RUN_FOREIGN)?;
        tasks::get_task(database, snapshot.run.task_id)
            .await
            .map_err(|_| RUN_UNREADABLE.to_string())?
            .filter(|task| task.workspace_id == scope.workspace_id)
            .ok_or(RUN_FOREIGN)?;

        let broadcaster = scope.state.task_progress().clone();
        let fresh = !broadcaster.tracks(run);
        let events = broadcaster.subscribe(run);
        // Read the status only once subscribed: a run that ends in between
        // would otherwise publish its ending to nobody and hang until the
        // deadline.
        let row = tasks::get_task_run(database, run)
            .await
            .map_err(|_| RUN_UNREADABLE.to_string())?
            .ok_or(RUN_FOREIGN)?;
        let finished = Finished::read(&row);
        if finished.is_some() && fresh {
            broadcaster.remove(run);
        }
        Ok((
            task_run_subject(run),
            None,
            Subscription::Run(Run {
                run,
                events,
                finished,
                database: database.clone(),
            }),
        ))
    }

    /// Resolve the reference now, so a typo costs a tool error rather than a
    /// park, and settle now if the commit's checks are already in.
    async fn commit(
        &self,
        request: &Request,
    ) -> Result<(String, Option<String>, Subscription), String> {
        let scope = self.scope.as_ref().ok_or(WORKSPACE_REQUIRED)?;
        let source = Uuid::parse_str(&request.id).map_err(|_| SOURCE_ID_INVALID.to_string())?;
        let github = self.github(scope, source).await?;
        let (reference, sha, assessment) = github.settled(request.reference.as_deref()).await?;
        Ok((
            check_subject(&reference, &sha),
            Some(reference.clone()),
            Subscription::Commit(Commit {
                checks: Box::new(github),
                reference,
                sha,
                assessment,
            }),
        ))
    }

    async fn github(&self, scope: &WorkspaceScope, source: Uuid) -> Result<Github, String> {
        let database = scope.state.db();
        if !workspace_members::can_read(database, scope.workspace_id, scope.user_id)
            .await
            .map_err(|_| WORKSPACE_UNREADABLE.to_string())?
        {
            return Err(WORKSPACE_UNREADABLE.to_string());
        }
        let source = sources::get_source(database, source, scope.workspace_id)
            .await
            .map_err(|_| SOURCE_UNREADABLE.to_string())?
            .filter(|source| source.is_active.unwrap_or(true))
            .ok_or(SOURCE_FOREIGN)?;
        if source.source_type != "github" {
            return Err(SOURCE_NOT_GITHUB.to_string());
        }
        let mut configuration: Configuration = serde_json::from_value(source.config)
            .map_err(|_| SOURCE_CONFIGURATION_INVALID.to_string())?;
        if let Some(encrypted) = source.credentials_encrypted {
            configuration.token = Some(
                crate::crypto::decrypt(scope.state.encryption_key(), &encrypted)
                    .map_err(|_| SOURCE_CREDENTIALS_INVALID.to_string())?,
            );
        }
        Github::new(configuration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::state::{AppState, test_config};
    use std::collections::{HashMap, VecDeque};
    use std::path::Path;
    use std::sync::Mutex as Lock;
    use tempfile::TempDir;
    use zone_core::context;
    use zone_core::llm::Role;
    use zone_core::tools::job::{JobCommand, JobStarted};

    const POLL: Duration = Duration::from_millis(20);
    const POLL_LIMIT: usize = 500;
    const CALL: &str = "call_7";

    /// The id a local model mints for the first call of a turn, which is why
    /// two sessions can hand the registry the same one.
    const SHARED_CALL: &str = "call_1";
    const FAR: Duration = Duration::from_secs(30);

    fn chat() -> Session {
        Session::Chat(Uuid::new_v4())
    }

    fn tool() -> WaitForTool {
        WaitForTool { scope: None }
    }

    fn scoped(state: AppState, workspace: Uuid, user: Uuid) -> WaitForTool {
        WaitForTool {
            scope: Some(WorkspaceScope {
                state,
                workspace_id: workspace,
                chat_id: Some(Uuid::new_v4()),
                user_id: user,
            }),
        }
    }

    fn context(session: Session) -> ToolContext {
        ToolContext {
            session,
            ..Default::default()
        }
    }

    fn environment() -> HashMap<String, String> {
        HashMap::from([(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )])
    }

    async fn spawned(session: Session, line: &str, cwd: &Path) -> JobStarted {
        Jobs::spawn(session, &JobCommand::shell(line), cwd, &environment())
            .await
            .expect("the job starts")
    }

    async fn settles(session: Session, id: &str) {
        for _ in 0..POLL_LIMIT {
            if Jobs::read(session, id, 0, 1)
                .await
                .expect("its own session reads it")
                .state
                .settled()
            {
                return;
            }
            tokio::time::sleep(POLL).await;
        }
        panic!("{id} never settled");
    }

    /// Open a wait and tie it to [`CALL`] under the session that opened it, the
    /// way the loop does. Answers with what the bound wait is for, which is
    /// the only place the clamped deadline is readable from.
    async fn opened(
        tool: &WaitForTool,
        session: Session,
        request: Value,
    ) -> (ToolResult, Option<Waiting>) {
        let result = tool
            .execute(request, &context(session))
            .await
            .expect("wait_for reports refusals as tool errors");
        let waiting = result
            .success
            .then(|| bind(session, CALL).expect("execute registered the wait it acknowledged"));
        (result, waiting)
    }

    fn refused(result: &ToolResult) -> String {
        assert!(!result.success, "expected a refusal, got {result:?}");
        result.error.clone().expect("a refusal carries its reason")
    }

    fn receipted(result: &ToolResult) -> String {
        assert!(result.success, "expected a receipt, got {result:?}");
        result.output.clone().expect("a receipt carries its text")
    }

    /// A commit whose checks answer from a script rather than from GitHub. A
    /// script that has run out goes on reporting nothing, which is what an
    /// empty check list answers.
    struct Scripted(Lock<VecDeque<Result<&'static str, String>>>);

    #[async_trait]
    impl Checks for Scripted {
        async fn assess(&self, _sha: &str) -> Result<&'static str, String> {
            self.0
                .lock()
                .expect("the script is not poisoned")
                .pop_front()
                .unwrap_or(Ok(UNKNOWN_ASSESSMENT))
        }
    }

    /// Checks no poll can read, the way an outage or an exhausted rate limit
    /// answers every one of them.
    struct Unreadable(&'static str);

    #[async_trait]
    impl Checks for Unreadable {
        async fn assess(&self, _sha: &str) -> Result<&'static str, String> {
            Err(self.0.to_string())
        }
    }

    fn commit(assessment: &'static str, polls: &[&'static str]) -> Commit {
        let script = polls.iter().copied().map(Ok).collect();
        watching(assessment, Box::new(Scripted(Lock::new(script))))
    }

    fn watching(assessment: &'static str, checks: Box<dyn Checks>) -> Commit {
        Commit {
            checks,
            reference: "main".to_string(),
            sha: "8c4d21fa9b7e6053".to_string(),
            assessment,
        }
    }

    /// What a GitHub read answers with when it fails. Shaped like the client's
    /// own messages, which is all this needs: the outcome quotes whatever
    /// reason it is handed.
    const UNREADABLE_REASON: &str = "GitHub returned HTTP 503.";

    /// Anything a model could read as "it passed".
    const SUCCESS_WORDS: [&str; 5] = ["success", "succeeded", "completed", "passed", "done"];

    fn assert_unphrasable_as_success(outcome: &str) {
        let lowered = outcome.to_lowercase();
        for word in SUCCESS_WORDS {
            assert!(
                !lowered.contains(word),
                "{outcome:?} contains {word:?}, which a model can read as a pass"
            );
        }
    }

    #[test]
    fn a_receipt_names_the_subject_and_the_deadline() {
        assert_eq!(
            receipt(&job_subject("job_9f3c1a7b2e04"), "2026-09-12T10:15:00Z"),
            "Waiting for job_9f3c1a7b2e04 until 2026-09-12T10:15:00Z."
        );
    }

    #[test]
    fn each_kind_of_subject_reads_as_itself() {
        let run = Uuid::parse_str("2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f").unwrap();
        assert_eq!(job_subject("job_9f3c1a7b2e04"), "job_9f3c1a7b2e04");
        assert_eq!(
            task_run_subject(run),
            "task run 2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f"
        );
        assert_eq!(
            check_subject("main", "8c4d21fa9b7e6053"),
            "checks on main (8c4d21f)"
        );
    }

    #[test]
    fn a_commit_shorter_than_the_quoted_prefix_is_quoted_whole() {
        assert_eq!(check_subject("main", "8c4d"), "checks on main (8c4d)");
    }

    #[test]
    fn a_job_outcome_reports_the_exit_code_and_the_time_it_took() {
        assert_eq!(
            job_exited("job_9f3c1a7b2e04", 0, Duration::from_secs(214)).text,
            "job_9f3c1a7b2e04 exited with code 0 after 214s."
        );
        assert_eq!(
            job_exited("job_9f3c1a7b2e04", 1, Duration::from_secs(214)).text,
            "job_9f3c1a7b2e04 exited with code 1 after 214s."
        );
    }

    #[test]
    fn a_killed_job_says_it_never_exited() {
        let outcome = job_killed("job_9f3c1a7b2e04", Duration::from_secs(900));
        assert_eq!(
            outcome.text,
            "job_9f3c1a7b2e04 was killed after 900s without exiting."
        );
        assert_unphrasable_as_success(&outcome.text);
    }

    #[test]
    fn a_task_run_outcome_reports_the_status_and_any_error() {
        let run = Uuid::parse_str("2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f").unwrap();
        assert_eq!(
            task_run_completed(run, Duration::from_secs(512)).text,
            "Task run 2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f completed after 512s."
        );
        assert_eq!(
            task_run_failed(run, Duration::from_secs(512), "the build broke").text,
            "Task run 2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f failed after 512s: the build broke"
        );
    }

    #[test]
    fn a_settled_check_names_what_it_settled_to() {
        assert_eq!(
            checks_settled(
                "main",
                "8c4d21fa9b7e6053",
                "success",
                Duration::from_secs(380)
            )
            .text,
            "Checks on main (8c4d21f) settled to success after 380s."
        );
        assert_eq!(
            checks_settled(
                "main",
                "8c4d21fa9b7e6053",
                "failure",
                Duration::from_secs(380)
            )
            .text,
            "Checks on main (8c4d21f) settled to failure after 380s."
        );
    }

    #[test]
    fn a_commit_nothing_reports_on_is_not_a_pass() {
        let outcome = checks_unknown("main", CHECK_SETTLE_GRACE);
        assert_eq!(
            outcome.text,
            "No checks are configured or reporting on main after 120s. This is not a pass."
        );
        assert_unphrasable_as_success(&outcome.text);
    }

    #[test]
    fn a_timeout_says_it_is_not_a_result() {
        let outcome = timed_out(&job_subject("job_9f3c1a7b2e04"), Duration::from_secs(300));
        assert_eq!(
            outcome.text,
            "Timed out after 300s. job_9f3c1a7b2e04 has not finished — this is a timeout, \
             not a result. Check again or wait longer."
        );
        assert_unphrasable_as_success(&outcome.text);
    }

    #[test]
    fn every_subject_times_out_without_reading_as_a_pass() {
        let run = Uuid::parse_str("2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f").unwrap();
        for subject in [
            job_subject("job_9f3c1a7b2e04"),
            task_run_subject(run),
            check_subject("main", "8c4d21fa9b7e6053"),
        ] {
            assert_unphrasable_as_success(&timed_out(&subject, Duration::from_secs(300)).text);
        }
    }

    #[test]
    fn the_wait_limit_error_quotes_the_limit_it_enforces() {
        assert_eq!(
            too_many_waits(),
            "You have waited 10 times in this attempt, which is the limit. \
             Act on what you already have."
        );
    }

    #[test]
    fn a_task_run_is_told_which_kinds_it_may_still_wait_on() {
        assert_eq!(
            task_run_wait_unavailable(),
            "Waiting on another task run is not available from a task run. Use kind=job or \
             kind=check, or finish and let whoever started this run coordinate."
        );
    }

    #[test]
    fn a_waiting_card_omits_a_reference_it_does_not_have() {
        let waiting = Waiting {
            kind: KIND_JOB.to_string(),
            id: "job_9f3c1a7b2e04".to_string(),
            reference: None,
            deadline: "2026-09-12T10:15:00Z".to_string(),
        };
        let json = serde_json::to_value(&waiting).unwrap();
        assert!(json.get("reference").is_none(), "{json}");
        assert_eq!(
            serde_json::from_value::<Waiting>(json).unwrap(),
            waiting,
            "the console's card round-trips through message metadata"
        );
    }

    /// The console is told which way the wait ended rather than left to read it
    /// out of the sentence, so the verdict has to survive the wire as a value
    /// of its own.
    #[test]
    fn a_settled_wait_says_which_way_it_ended() {
        let outcome = timed_out(&job_subject("job_9f3c1a7b2e04"), Duration::from_secs(300));
        let settled = WaitSettled {
            tool_call_id: "call_7".to_string(),
            outcome: outcome.text,
            verdict: outcome.verdict,
        };
        let json = serde_json::to_value(&settled).unwrap();
        assert_eq!(json["verdict"], "timed_out", "{json}");
        assert_eq!(
            serde_json::from_value::<WaitSettled>(json).unwrap(),
            settled
        );
    }

    /// Every value the console models, in the spelling it models them in. A
    /// variant renamed on this side alone parses as nothing on that one, and a
    /// card that cannot read the verdict stays drawn as a wait still running.
    #[test]
    fn every_verdict_travels_as_the_console_spells_it() {
        for (verdict, wire) in [
            (Verdict::Settled, "settled"),
            (Verdict::TimedOut, "timed_out"),
            (Verdict::Silent, "silent"),
            (Verdict::Unreadable, "unreadable"),
        ] {
            let json = serde_json::to_value(verdict).unwrap();
            assert_eq!(json, wire);
            assert_eq!(serde_json::from_value::<Verdict>(json).unwrap(), verdict);
        }
    }

    #[test]
    fn the_schema_publishes_the_window_it_clamps_to() {
        let schema = tool().parameters_schema();
        let timeout = &schema["properties"]["timeout_secs"];
        assert_eq!(
            timeout["minimum"], MIN_WAIT_SECS,
            "a model that cannot see the minimum will ask for a poll: {schema}"
        );
        assert_eq!(timeout["maximum"], MAX_WAIT_SECS, "{schema}");
        assert_eq!(
            schema["properties"]["kind"]["enum"],
            json!([KIND_JOB, KIND_TASK_RUN, KIND_CHECK]),
            "{schema}"
        );
        assert_eq!(schema["required"], json!(["kind", "id"]), "{schema}");
        assert_eq!(schema["additionalProperties"], json!(false), "{schema}");
    }

    #[test]
    fn a_wait_ends_the_turn_and_keeps_the_default_tool_timeout() {
        let tool = tool();
        assert!(tool.ends_turn());
        assert_eq!(tool.tier(), Tier::Read);
        assert_eq!(
            tool.timeout(&context(chat())),
            Duration::from_secs(30),
            "execute only registers, so the outer bound has nothing to accommodate"
        );
    }

    #[test]
    fn a_requested_window_is_clamped_to_what_the_schema_advertises() {
        let session = chat();
        assert_eq!(
            window(None, session),
            Some(Duration::from_secs(DEFAULT_WAIT_SECS))
        );
        assert_eq!(
            window(Some(1), session),
            Some(Duration::from_secs(MIN_WAIT_SECS))
        );
        assert_eq!(
            window(Some(u64::MAX), session),
            Some(Duration::from_secs(MAX_WAIT_SECS))
        );
        assert_eq!(window(Some(900), session), Some(Duration::from_secs(900)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_window_is_clamped_again_to_the_ceiling_the_surface_owns() {
        let session = chat();
        set_ceiling(session, Instant::now() + Duration::from_secs(60));
        assert_eq!(
            window(None, session),
            Some(Duration::from_secs(60)),
            "a chat turn cannot outlast its own stream deadline"
        );
        set_ceiling(
            session,
            Instant::now() + Duration::from_secs(MIN_WAIT_SECS - 1),
        );
        assert_eq!(
            window(None, session),
            None,
            "a window under the floor is refused rather than clamped to nothing"
        );
        reset_session(session);
        assert_eq!(
            window(None, session),
            Some(Duration::from_secs(DEFAULT_WAIT_SECS)),
            "resetting clears the ceiling with the counter"
        );
    }

    #[tokio::test]
    async fn a_job_that_exits_during_the_wait_reports_its_code() {
        let directory = TempDir::new().expect("a temporary working directory");
        let session = chat();
        let job = spawned(session, "sleep 0.2; exit 0", directory.path()).await;

        let (opened, waiting) =
            opened(&tool(), session, json!({"kind": KIND_JOB, "id": &job.id})).await;
        let deadline = waiting.expect("a receipt means a bound wait").deadline;
        assert_eq!(
            receipted(&opened),
            receipt(&job_subject(&job.id), &deadline)
        );

        let outcome = await_outcome(session, CALL, Instant::now() + FAR).await;
        assert_eq!(outcome, job_exited(&job.id, 0, Duration::ZERO));
        reset_session(session);
    }

    #[tokio::test]
    async fn a_job_that_had_already_exited_is_reported_at_once() {
        let directory = TempDir::new().expect("a temporary working directory");
        let session = chat();
        let job = spawned(session, "exit 1", directory.path()).await;
        settles(session, &job.id).await;

        opened(&tool(), session, json!({"kind": KIND_JOB, "id": &job.id})).await;
        let started = std::time::Instant::now();
        let outcome = await_outcome(session, CALL, Instant::now() + FAR).await;
        assert_eq!(outcome, job_exited(&job.id, 1, Duration::ZERO));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a job that ended before the wait was issued must not wait out its deadline"
        );
        reset_session(session);
    }

    #[tokio::test]
    async fn opening_a_job_wait_returns_well_inside_the_tool_timeout() {
        let directory = TempDir::new().expect("a temporary working directory");
        let session = chat();
        let job = spawned(session, "sleep 30", directory.path()).await;

        let started = std::time::Instant::now();
        let (opened, _) = opened(&tool(), session, json!({"kind": KIND_JOB, "id": &job.id})).await;
        let elapsed = started.elapsed();
        assert!(opened.success, "{opened:?}");
        assert!(
            elapsed < Duration::from_secs(1),
            "execute authorizes and registers only, but took {elapsed:?}"
        );
        reset_session(session);
        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn a_wait_that_runs_out_reports_a_timeout_rather_than_a_result() {
        let directory = TempDir::new().expect("a temporary working directory");
        let session = chat();
        let job = spawned(session, "sleep 30", directory.path()).await;

        opened(&tool(), session, json!({"kind": KIND_JOB, "id": &job.id})).await;
        let outcome = await_outcome(session, CALL, Instant::now()).await;
        assert_eq!(outcome, timed_out(&job_subject(&job.id), Duration::ZERO));
        assert_unphrasable_as_success(&outcome.text);
        reset_session(session);
        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn a_job_from_another_session_is_refused() {
        let directory = TempDir::new().expect("a temporary working directory");
        let owner = chat();
        let stranger = chat();
        let job = spawned(owner, "sleep 30", directory.path()).await;

        let refusal = tool()
            .execute(json!({"kind": KIND_JOB, "id": &job.id}), &context(stranger))
            .await
            .unwrap();
        assert!(refused(&refusal).contains(&job.id));
        assert_eq!(waits_taken(stranger), 0, "a refused wait is not a wait");
        Jobs::kill_session(owner).await;
    }

    #[tokio::test]
    async fn waiting_on_a_task_run_is_refused_from_inside_a_task_run() {
        let session = Session::Task(Uuid::new_v4());
        let refusal = tool()
            .execute(
                json!({"kind": KIND_TASK_RUN, "id": Uuid::new_v4().to_string()}),
                &context(session),
            )
            .await
            .unwrap();
        assert_eq!(refused(&refusal), task_run_wait_unavailable());
    }

    #[tokio::test]
    async fn the_eleventh_wait_in_an_attempt_is_an_error_rather_than_a_park() {
        let directory = TempDir::new().expect("a temporary working directory");
        let session = chat();
        let job = spawned(session, "sleep 30", directory.path()).await;
        let request = json!({"kind": KIND_JOB, "id": &job.id});

        for taken in 0..MAX_WAITS_PER_ATTEMPT {
            assert_eq!(waits_taken(session), taken);
            let opened = tool()
                .execute(request.clone(), &context(session))
                .await
                .unwrap();
            assert!(opened.success, "wait {taken} was refused: {opened:?}");
        }
        let refusal = tool()
            .execute(request.clone(), &context(session))
            .await
            .unwrap();
        assert_eq!(refused(&refusal), too_many_waits());

        reset_session(session);
        assert_eq!(waits_taken(session), 0);
        let after = tool().execute(request, &context(session)).await.unwrap();
        assert!(after.success, "a reset attempt starts over: {after:?}");
        reset_session(session);
        Jobs::kill_session(session).await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_commit_whose_checks_settle_reports_what_they_settled_to() {
        for assessment in SETTLED_ASSESSMENTS {
            let started = Instant::now();
            let outcome = commit("pending", &[assessment]).settle(started).await;
            assert_eq!(
                outcome,
                checks_settled("main", "8c4d21fa9b7e6053", assessment, CHECK_POLL_INTERVAL)
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_commit_whose_checks_had_already_settled_is_reported_at_once() {
        let started = Instant::now();
        let outcome = commit("success", &[]).settle(started).await;
        assert_eq!(
            outcome,
            checks_settled("main", "8c4d21fa9b7e6053", "success", Duration::ZERO),
            "a commit already green when the wait opened must not wait out a poll"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_commit_nothing_reports_on_ends_the_wait_without_reading_as_a_pass() {
        let outcome = commit(UNKNOWN_ASSESSMENT, &[]).settle(Instant::now()).await;
        assert_eq!(outcome, checks_unknown("main", CHECK_SETTLE_GRACE));
        assert_unphrasable_as_success(&outcome.text);
    }

    /// A poll GitHub refused is not a commit nothing is reporting on. Ending
    /// the grace period with "no checks are configured" blames the repository
    /// for an outage, and a model reads it as a repository that does not test
    /// itself rather than as evidence it never got.
    #[tokio::test(start_paused = true)]
    async fn checks_that_could_not_be_read_end_the_wait_as_an_outage_not_as_silence() {
        let outcome = watching(UNKNOWN_ASSESSMENT, Box::new(Unreadable(UNREADABLE_REASON)))
            .settle(Instant::now())
            .await;

        assert_eq!(
            outcome,
            checks_unreadable("main", UNREADABLE_REASON, CHECK_SETTLE_GRACE)
        );
        assert_unphrasable_as_success(&outcome.text);
    }

    /// Which of the two a wait ends with is the *last* poll's answer: an
    /// outage that clears and leaves a commit still unreported on is the empty
    /// case again, and saying the checks could not be read would be a stale
    /// complaint about a read that has since succeeded.
    #[tokio::test(start_paused = true)]
    async fn a_failed_poll_that_recovers_ends_the_wait_as_silence_again() {
        let script = VecDeque::from([Err(UNREADABLE_REASON.to_string())]);
        let outcome = watching(UNKNOWN_ASSESSMENT, Box::new(Scripted(Lock::new(script))))
            .settle(Instant::now())
            .await;

        assert_eq!(outcome, checks_unknown("main", CHECK_SETTLE_GRACE));
    }

    #[tokio::test(start_paused = true)]
    async fn a_commit_that_starts_reporting_again_restarts_the_grace_period() {
        let unknown = UNKNOWN_ASSESSMENT;
        let polls = [
            unknown, unknown, "pending", unknown, unknown, unknown, "success",
        ];
        let outcome = commit(unknown, &polls).settle(Instant::now()).await;
        assert_eq!(
            outcome,
            checks_settled(
                "main",
                "8c4d21fa9b7e6053",
                "success",
                CHECK_POLL_INTERVAL * polls.len() as u32
            ),
            "the grace period measures continuous silence, so one reporting poll restarts it"
        );
    }

    #[tokio::test]
    async fn a_directly_registered_wait_settles_under_its_call_and_ends_at_its_deadline() {
        let session = chat();
        let run = Uuid::new_v4();
        let waiting = Waiting {
            kind: KIND_TASK_RUN.to_string(),
            id: run.to_string(),
            reference: None,
            deadline: "2026-09-12T10:15:00Z".to_string(),
        };
        claim("call_9", waiting, session);
        assert_eq!(
            await_outcome(session, "call_9", Instant::now()).await,
            timed_out(&task_run_subject(run), Duration::ZERO),
            "a wait with nothing behind it still ends, and says it did not finish"
        );
        assert_eq!(
            await_outcome(session, "call_9", Instant::now()).await,
            timed_out(LOST_SUBJECT, Duration::ZERO),
            "settling a wait releases it, so a second await cannot resolve it again"
        );
        reset_session(session);
    }

    /// A surface whose own deadline has all but elapsed leaves no window to
    /// wait in. Clamping to what is left would mint one that expires the
    /// moment it is awaited, and the receipt acknowledging it reads as a
    /// success while spending one of the ten allowances.
    #[tokio::test]
    async fn a_wait_the_surface_has_no_window_left_for_is_refused() {
        let directory = TempDir::new().expect("a temporary working directory");
        let session = chat();
        let job = spawned(session, "exit 0", directory.path()).await;
        settles(session, &job.id).await;
        set_ceiling(
            session,
            Instant::now() + Duration::from_secs(MIN_WAIT_SECS - 1),
        );

        let refusal = tool()
            .execute(json!({"kind": KIND_JOB, "id": &job.id}), &context(session))
            .await
            .expect("wait_for reports refusals as tool errors");

        assert_eq!(refused(&refusal), no_window_left());
        assert_eq!(
            waits_taken(session),
            0,
            "a refused wait must not spend an allowance"
        );
        assert_eq!(
            bind(session, CALL),
            None,
            "a refused wait must leave nothing staged for the loop to park on"
        );
        reset_session(session);
    }

    /// Nothing makes a tool-call id unique across sessions: a local model emits
    /// `call_1` for the first call of every turn, so two concurrent sessions
    /// hand the registry one id and each must still settle its own subject.
    #[tokio::test]
    async fn two_sessions_waiting_under_one_call_id_each_settle_their_own() {
        let directory = TempDir::new().expect("a temporary working directory");
        let first = chat();
        let second = chat();
        let theirs = spawned(first, "exit 3", directory.path()).await;
        let other = spawned(second, "exit 4", directory.path()).await;
        settles(first, &theirs.id).await;
        settles(second, &other.id).await;

        for (session, job) in [(first, &theirs), (second, &other)] {
            let opened = tool()
                .execute(json!({"kind": KIND_JOB, "id": &job.id}), &context(session))
                .await
                .expect("wait_for reports refusals as tool errors");
            assert!(opened.success, "{opened:?}");
            assert_eq!(
                bind(session, SHARED_CALL)
                    .map(|waiting| waiting.id)
                    .as_deref(),
                Some(job.id.as_str()),
                "a session binds the wait it opened, never another session's"
            );
        }

        assert_eq!(
            await_outcome(first, SHARED_CALL, Instant::now() + FAR).await,
            job_exited(&theirs.id, 3, Duration::ZERO),
            "the first waiter was handed the other session's subject"
        );
        assert_eq!(
            await_outcome(second, SHARED_CALL, Instant::now() + FAR).await,
            job_exited(&other.id, 4, Duration::ZERO),
            "the second waiter lost its own wait to the first"
        );
        reset_session(first);
        reset_session(second);
    }

    /// A session's teardown clears the waits that session opened. Under one
    /// key per call id it cleared whatever another session held under the same
    /// id, having already replaced it.
    #[tokio::test]
    async fn resetting_one_session_leaves_another_waiting_under_the_same_id() {
        let directory = TempDir::new().expect("a temporary working directory");
        let waiting = chat();
        let torn_down = chat();
        let job = spawned(waiting, "exit 5", directory.path()).await;
        settles(waiting, &job.id).await;

        let opened = tool()
            .execute(json!({"kind": KIND_JOB, "id": &job.id}), &context(waiting))
            .await
            .expect("wait_for reports refusals as tool errors");
        assert!(opened.success, "{opened:?}");
        bind(waiting, SHARED_CALL).expect("the wait binds under the call that opened it");

        claim(
            SHARED_CALL,
            Waiting {
                kind: KIND_JOB.to_string(),
                id: "job_0e5a1c93b746".to_string(),
                reference: None,
                deadline: "2026-09-12T10:15:00Z".to_string(),
            },
            torn_down,
        );
        reset_session(torn_down);

        assert_eq!(
            await_outcome(waiting, SHARED_CALL, Instant::now() + FAR).await,
            job_exited(&job.id, 5, Duration::ZERO),
            "another session's teardown took this wait with it"
        );
        reset_session(waiting);
    }

    #[test]
    fn a_subject_is_readable_from_the_wait_alone() {
        let run = Uuid::parse_str("2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f").unwrap();
        let waiting = |kind: &str, id: &str, reference: Option<&str>| Waiting {
            kind: kind.to_string(),
            id: id.to_string(),
            reference: reference.map(str::to_string),
            deadline: "2026-09-12T10:15:00Z".to_string(),
        };
        assert_eq!(
            subject(&waiting(KIND_JOB, "job_9f3c1a7b2e04", None)),
            "job_9f3c1a7b2e04"
        );
        assert_eq!(
            subject(&waiting(KIND_TASK_RUN, &run.to_string(), None)),
            task_run_subject(run)
        );
        assert_eq!(
            subject(&waiting(KIND_CHECK, "a-source", Some("main"))),
            "checks on main"
        );
    }

    #[tokio::test]
    async fn a_wait_nobody_registered_settles_as_a_timeout() {
        assert_eq!(
            await_outcome(chat(), "call_that_never_waited", Instant::now() + FAR).await,
            timed_out(LOST_SUBJECT, Duration::ZERO)
        );
    }

    fn outcome_pair(context: &mut RunContext, waits: &mut Vec<String>, call: &str, outcome: &str) {
        let waited = Waited::new(
            call,
            Waiting {
                kind: KIND_JOB.to_string(),
                id: "job_9f3c1a7b2e04".to_string(),
                reference: None,
                deadline: "2026-09-12T10:15:00Z".to_string(),
            },
        );
        resume_with_outcome(context, &waited, outcome.to_string(), waits);
    }

    #[test]
    fn an_outcome_arrives_as_an_envelope_and_the_result_that_follows_it() {
        let mut context = RunContext::from_messages(vec![LlmMessage::user("Ship it.")]);
        let mut waits = Vec::new();
        let pair = {
            let waited = Waited::new(
                CALL,
                Waiting {
                    kind: KIND_JOB.to_string(),
                    id: "job_9f3c1a7b2e04".to_string(),
                    reference: None,
                    deadline: "2026-09-12T10:15:00Z".to_string(),
                },
            );
            resume_with_outcome(
                &mut context,
                &waited,
                job_exited("job_9f3c1a7b2e04", 0, Duration::from_secs(214)).text,
                &mut waits,
            )
        };

        let envelope = &pair[0].message;
        assert_eq!(envelope.role, Role::Assistant);
        let calls = envelope.tool_calls.as_ref().expect("a one-call envelope");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, format!("{CALL}{SETTLED_SUFFIX}"));
        assert_eq!(calls[0].function.name, WAIT_FOR);
        assert_eq!(
            serde_json::from_str::<Waiting>(&calls[0].function.arguments)
                .expect("the registered wait travels as the call's arguments")
                .id,
            "job_9f3c1a7b2e04"
        );

        let result = &pair[1].message;
        assert_eq!(result.role, Role::Tool);
        assert_eq!(
            result.tool_call_id.as_deref(),
            Some(format!("{CALL}{SETTLED_SUFFIX}").as_str())
        );
        assert_eq!(
            result.content.as_deref(),
            Some("job_9f3c1a7b2e04 exited with code 0 after 214s.")
        );

        let injected: Vec<&Entry> = context
            .entries
            .iter()
            .filter(|entry| waits.contains(&entry.id))
            .collect();
        assert_eq!(injected.len(), 2);
        for entry in injected {
            assert!(entry.preserve, "the newest outcome survives compaction");
            assert!(
                !entry.consumed,
                "the store records it unconsumed, and the next request reconciles both sides"
            );
        }
    }

    #[test]
    fn two_successive_outcome_pairs_group_cleanly_and_the_older_one_is_demoted() {
        let mut context = RunContext::from_messages(vec![LlmMessage::user("Ship it.")]);
        let mut waits = Vec::new();
        outcome_pair(&mut context, &mut waits, "call_1", "First outcome.");
        let older: Vec<String> = waits.clone();
        outcome_pair(&mut context, &mut waits, "call_2", "Second outcome.");

        context::validate(&context.entries, None)
            .expect("an envelope and the result that follows it is a complete group");

        for entry in &context.entries {
            if older.contains(&entry.id) {
                assert!(
                    !entry.preserve,
                    "the previous outcome is demoted so preserved evidence does not grow per park"
                );
            } else if waits.contains(&entry.id) {
                assert!(entry.preserve, "the newest outcome stays preserved");
            }
        }
        assert_eq!(waits.len(), 4, "both pairs are recorded: {waits:?}");
    }

    #[tokio::test]
    #[ignore = "requires migrated PostgreSQL via TEST_DATABASE_URL"]
    async fn a_task_run_settles_from_the_event_its_writers_publish() {
        let fixture = Fixture::create().await;
        let run = fixture.run(fixture.workspace).await;
        let session = chat();

        let started = std::time::Instant::now();
        let (opened, _) = opened(
            &fixture.tool(),
            session,
            json!({"kind": KIND_TASK_RUN, "id": run.to_string()}),
        )
        .await;
        let elapsed = started.elapsed();
        assert!(opened.success, "{opened:?}");
        assert!(
            elapsed < Duration::from_secs(1),
            "execute subscribes rather than waiting, but took {elapsed:?}"
        );
        assert!(receipted(&opened).contains(&task_run_subject(run)));

        tokio::spawn(async move {
            tokio::time::sleep(POLL).await;
            crate::services::task_progress::publish_terminal(run, "completed", None);
        });
        let outcome = await_outcome(session, CALL, Instant::now() + FAR).await;
        assert_eq!(outcome, task_run_completed(run, Duration::ZERO));
        reset_session(session);
    }

    #[tokio::test]
    #[ignore = "requires migrated PostgreSQL via TEST_DATABASE_URL"]
    async fn a_task_run_that_had_already_finished_is_reported_at_once() {
        let fixture = Fixture::create().await;
        let run = fixture.run(fixture.workspace).await;
        fixture.finish(run, "failed", Some("the build broke")).await;
        let session = chat();

        opened(
            &fixture.tool(),
            session,
            json!({"kind": KIND_TASK_RUN, "id": run.to_string()}),
        )
        .await;
        let started = std::time::Instant::now();
        let outcome = await_outcome(session, CALL, Instant::now() + FAR).await;
        assert_eq!(
            outcome,
            task_run_failed(run, Duration::ZERO, "the build broke")
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a run that ended before the wait was issued must not wait out its deadline"
        );
        reset_session(session);
    }

    #[tokio::test]
    #[ignore = "requires migrated PostgreSQL via TEST_DATABASE_URL"]
    async fn a_run_in_another_workspace_is_refused_to_one_of_its_own_members() {
        let fixture = Fixture::create().await;
        let foreign = fixture.workspace("foreign").await;
        workspace_members::add_member(
            &fixture.pool,
            foreign,
            fixture.user,
            workspace_members::WorkspaceRole::Member,
            None,
        )
        .await
        .expect("the caller joins the workspace that owns the run");
        let run = fixture.run(foreign).await;

        assert!(
            task_access::read(&fixture.pool, run, fixture.user)
                .await
                .expect("the membership read succeeds")
                .is_some(),
            "membership of the run's own workspace is exactly what must not be enough"
        );
        let refusal = fixture
            .tool()
            .execute(
                json!({"kind": KIND_TASK_RUN, "id": run.to_string()}),
                &context(chat()),
            )
            .await
            .unwrap();
        assert_eq!(refused(&refusal), RUN_FOREIGN);
    }

    /// A workspace, a member, and runs to wait on.
    struct Fixture {
        pool: PgPool,
        state: AppState,
        workspace: Uuid,
        user: Uuid,
        organization: Uuid,
        identifier: Uuid,
    }

    impl Fixture {
        async fn create() -> Self {
            use crate::db::{organizations, users, workspaces};
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL required"),
            )
            .await
            .expect("the test database accepts connections");
            let identifier = Uuid::new_v4();
            let organization = organizations::create_organization(
                &pool,
                "Wait test",
                &format!("wait-{identifier}"),
                None,
            )
            .await
            .unwrap()
            .id;
            let user = users::create_user(
                &pool,
                &format!("wait-{identifier}@example.test"),
                "hash",
                None,
                false,
            )
            .await
            .unwrap()
            .id;
            let workspace = workspaces::create_workspace(
                &pool,
                organization,
                "Own",
                &format!("own-{identifier}"),
                None,
            )
            .await
            .unwrap()
            .id;
            workspace_members::add_member(
                &pool,
                workspace,
                user,
                workspace_members::WorkspaceRole::Member,
                None,
            )
            .await
            .unwrap();
            let state = AppState::new(test_config(), pool.clone(), None);
            Self {
                pool,
                state,
                workspace,
                user,
                organization,
                identifier,
            }
        }

        fn tool(&self) -> WaitForTool {
            scoped(self.state.clone(), self.workspace, self.user)
        }

        async fn workspace(&self, name: &str) -> Uuid {
            crate::db::workspaces::create_workspace(
                &self.pool,
                self.organization,
                name,
                &format!("{name}-{}", self.identifier),
                None,
            )
            .await
            .unwrap()
            .id
        }

        async fn run(&self, workspace: Uuid) -> Uuid {
            let task = tasks::create_task(
                &self.pool,
                workspace,
                &[],
                "Wait",
                "Wait for something",
                None,
                None,
                true,
                None,
            )
            .await
            .unwrap();
            tasks::create_task_run_as(&self.pool, task.id, None)
                .await
                .unwrap()
                .id
        }

        async fn finish(&self, run: Uuid, status: &str, error: Option<&str>) {
            sqlx::query(
                "UPDATE task_runs SET status = $2, error_message = $3, completed_at = NOW() WHERE id = $1",
            )
            .bind(run)
            .bind(status)
            .bind(error)
            .execute(&self.pool)
            .await
            .expect("the fixture ends the run");
        }
    }
}
