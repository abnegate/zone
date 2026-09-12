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

use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

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
    pub timed_out: bool,
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

pub fn job_exited(id: &str, exit_code: i32, elapsed: Duration) -> String {
    format!(
        "{id} exited with code {exit_code} after {}s.",
        elapsed.as_secs()
    )
}

pub fn job_killed(id: &str, elapsed: Duration) -> String {
    format!(
        "{id} was killed after {}s without exiting.",
        elapsed.as_secs()
    )
}

pub fn task_run_completed(run: Uuid, elapsed: Duration) -> String {
    format!("Task run {run} completed after {}s.", elapsed.as_secs())
}

pub fn task_run_failed(run: Uuid, elapsed: Duration, error: &str) -> String {
    format!(
        "Task run {run} failed after {}s: {error}",
        elapsed.as_secs()
    )
}

pub fn checks_settled(reference: &str, sha: &str, assessment: &str, elapsed: Duration) -> String {
    format!(
        "Checks on {reference} ({}) settled to {assessment} after {}s.",
        short(sha),
        elapsed.as_secs()
    )
}

/// A commit nothing is reporting on, after [`CHECK_SETTLE_GRACE`]. Deliberately
/// says out loud that it is not a pass.
pub fn checks_unknown(reference: &str, elapsed: Duration) -> String {
    format!(
        "No checks are configured or reporting on {reference} after {}s. This is not a pass.",
        elapsed.as_secs()
    )
}

/// The wait ran out. Says what did *not* happen, because a model handed a bare
/// "finished" would act as though it had.
pub fn timed_out(subject: &str, waited: Duration) -> String {
    format!(
        "Timed out after {}s. {subject} has not finished — this is a timeout, not a result. \
         Check again or wait longer.",
        waited.as_secs()
    )
}

/// Returned as a tool error, so the turn never ends and nothing parks.
pub fn too_many_waits() -> String {
    format!(
        "You have waited {MAX_WAITS_PER_ATTEMPT} times in this attempt, which is the limit. \
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

/// Every open wait on this instance, keyed by the tool call that opened it.
///
/// Process-local and not persisted, the same single-instance assumption the run
/// socket and the question waiter already document: what is being waited on is
/// a child process or a subscription this process holds, so a surviving row
/// would claim a durability the thing does not have.
static REGISTERED: Lazy<DashMap<String, Waiting>> = Lazy::new(DashMap::new);

/// What a tool call is waiting for, for a caller that holds only its id.
///
/// The loop reads the clamped deadline from here rather than re-parsing the
/// call's arguments, because the clamp is `wait_for::execute`'s to apply.
pub fn registered(tool_call_id: &str) -> Option<Waiting> {
    REGISTERED.get(tool_call_id).map(|entry| entry.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            job_exited("job_9f3c1a7b2e04", 0, Duration::from_secs(214)),
            "job_9f3c1a7b2e04 exited with code 0 after 214s."
        );
        assert_eq!(
            job_exited("job_9f3c1a7b2e04", 1, Duration::from_secs(214)),
            "job_9f3c1a7b2e04 exited with code 1 after 214s."
        );
    }

    #[test]
    fn a_killed_job_says_it_never_exited() {
        let outcome = job_killed("job_9f3c1a7b2e04", Duration::from_secs(900));
        assert_eq!(
            outcome,
            "job_9f3c1a7b2e04 was killed after 900s without exiting."
        );
        assert_unphrasable_as_success(&outcome);
    }

    #[test]
    fn a_task_run_outcome_reports_the_status_and_any_error() {
        let run = Uuid::parse_str("2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f").unwrap();
        assert_eq!(
            task_run_completed(run, Duration::from_secs(512)),
            "Task run 2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f completed after 512s."
        );
        assert_eq!(
            task_run_failed(run, Duration::from_secs(512), "the build broke"),
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
            ),
            "Checks on main (8c4d21f) settled to success after 380s."
        );
        assert_eq!(
            checks_settled(
                "main",
                "8c4d21fa9b7e6053",
                "failure",
                Duration::from_secs(380)
            ),
            "Checks on main (8c4d21f) settled to failure after 380s."
        );
    }

    #[test]
    fn a_commit_nothing_reports_on_is_not_a_pass() {
        let outcome = checks_unknown("main", CHECK_SETTLE_GRACE);
        assert_eq!(
            outcome,
            "No checks are configured or reporting on main after 120s. This is not a pass."
        );
        assert_unphrasable_as_success(&outcome);
    }

    #[test]
    fn a_timeout_says_it_is_not_a_result() {
        let outcome = timed_out(&job_subject("job_9f3c1a7b2e04"), Duration::from_secs(300));
        assert_eq!(
            outcome,
            "Timed out after 300s. job_9f3c1a7b2e04 has not finished — this is a timeout, \
             not a result. Check again or wait longer."
        );
        assert_unphrasable_as_success(&outcome);
    }

    #[test]
    fn every_subject_times_out_without_reading_as_a_pass() {
        let run = Uuid::parse_str("2f1c9e8a-0b44-4d7e-9c31-5a6b7c8d9e0f").unwrap();
        for subject in [
            job_subject("job_9f3c1a7b2e04"),
            task_run_subject(run),
            check_subject("main", "8c4d21fa9b7e6053"),
        ] {
            assert_unphrasable_as_success(&timed_out(&subject, Duration::from_secs(300)));
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
    fn a_wait_nobody_registered_is_not_found() {
        assert_eq!(registered("call_that_never_waited"), None);
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

    #[test]
    fn a_settled_wait_says_whether_it_timed_out() {
        let settled = WaitSettled {
            tool_call_id: "call_7".to_string(),
            outcome: timed_out(&job_subject("job_9f3c1a7b2e04"), Duration::from_secs(300)),
            timed_out: true,
        };
        let json = serde_json::to_value(&settled).unwrap();
        assert_eq!(json["timed_out"], true, "{json}");
        assert_eq!(
            serde_json::from_value::<WaitSettled>(json).unwrap(),
            settled
        );
    }
}
