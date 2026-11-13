//! What each chat in a workspace is doing, for the list to show at a glance.
//!
//! Two independent readings per chat, because they answer different questions
//! and a chat can be in both: what the chat itself is doing — writing a reply,
//! or stopped and waiting for the reader — and what became of the pull request
//! its work opened.
//!
//! Neither reading invents a state it could not establish. A status GitHub
//! could not be asked for stays absent rather than becoming "passing", and
//! "ready to merge" is only ever said of a pull request every part of which was
//! read: see [`Status::ready_to_merge`].

use futures::{StreamExt, stream};
use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::agent::ApprovalPolicy;
use crate::db::chat_pull_requests::{self, PullRequestRow};
use crate::db::chats::activity;
use crate::db::{DbResult, chats};
use crate::state::AppState;

use super::pull_request::{self, Checks, State, Status};

/// How old a reading of a pull request the list will show rather than asking
/// GitHub for a new one.
///
/// Much wider than the panel's window on purpose. A panel is one pull request
/// somebody is looking at; a list is every pull request in the workspace, and
/// refreshing all of them at the panel's rate would spend the workspace's
/// GitHub budget on rows nobody is reading.
const LIST_FRESHNESS: Duration = Duration::from_secs(300);

/// The longest one list read spends asking GitHub for statuses it has no fresh
/// reading of.
///
/// What the budget runs out on is reported as unread rather than waited for;
/// the next read picks it up with a warm cache behind it.
const REFRESH_BUDGET: Duration = Duration::from_secs(8);

/// How many pull requests are refreshed at once.
const CONCURRENT_REFRESHES: usize = 4;

/// What a chat is doing, as far as the reader is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// A turn is in flight and nobody is being waited on.
    Writing,
    /// A turn is parked on a tool call the reader has to approve or deny.
    AwaitingApproval,
    /// The chat's last word was a question with no answer after it.
    AwaitingAnswer,
    /// Nothing in flight and nothing outstanding.
    Idle,
}

impl Activity {
    /// Which of the three readings the row shows.
    ///
    /// Waiting for a person outranks writing: an approval card holds the turn
    /// still, so a chat parked on one is not making progress however live its
    /// lease is. A question only counts when no turn is running, because a turn
    /// starting means the user message that answers it was already stored.
    pub fn read(writing: bool, awaiting_approval: bool, awaiting_answer: bool) -> Self {
        match (writing, awaiting_approval, awaiting_answer) {
            (_, true, _) => Self::AwaitingApproval,
            (true, false, _) => Self::Writing,
            (false, false, true) => Self::AwaitingAnswer,
            (false, false, false) => Self::Idle,
        }
    }

    /// Whether this chat has stopped and is waiting for a person.
    pub fn waiting(self) -> bool {
        matches!(self, Self::AwaitingApproval | Self::AwaitingAnswer)
    }
}

/// The pull request a chat's row speaks for, and what GitHub last said of it.
#[derive(Debug, Clone, Serialize)]
pub struct PullIndicator {
    pub id: Uuid,
    pub number: i64,
    pub url: String,
    pub title: Option<String>,
    /// `None` when no source can read it, GitHub could not be asked, or this
    /// read had no budget left to ask with.
    pub status: Option<Status>,
    /// Whether it could be merged as it stands, by the whole of
    /// [`Status::ready_to_merge`]. False whenever any part of that is unread.
    pub ready: bool,
}

impl PullIndicator {
    fn new(row: &PullRequestRow, status: Option<Status>) -> Self {
        Self {
            id: row.id,
            number: row.number,
            url: row.url.clone(),
            title: status
                .as_ref()
                .map(|status| status.title.clone())
                .or_else(|| row.title.clone()),
            ready: status
                .as_ref()
                .is_some_and(|status| status.ready_to_merge()),
            status,
        }
    }

    /// Where this pull request comes in the queue for a reader's attention,
    /// lowest first. Used to choose which of a chat's pull requests its single
    /// row speaks for.
    fn rank(&self) -> u8 {
        let Some(status) = &self.status else {
            // Unread, so it cannot be ranked above anything a reader could act
            // on, and must not be ranked below a settled one and disappear.
            return 4;
        };
        match status.state {
            State::Merged => 6,
            State::Closed => 7,
            State::Open if status.draft => 5,
            State::Open => match status.checks {
                Checks::Failing => 0,
                Checks::Pending => 1,
                _ if self.ready => 3,
                _ => 2,
            },
        }
    }
}

/// One chat as its row shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Indicator {
    pub chat_id: Uuid,
    pub activity: Activity,
    /// The pull request this row speaks for: of the chat's undismissed ones,
    /// whichever wants a reader's attention first.
    pub pull_request: Option<PullIndicator>,
    /// How many undismissed pull requests the chat has, so a row speaking for
    /// one of several can say so.
    pub pull_requests: usize,
}

/// Read every chat in the workspace, in the order the chat list asks for them.
pub async fn list(
    state: &AppState,
    workspace_id: Uuid,
    archived: Option<bool>,
) -> DbResult<Vec<Indicator>> {
    let chats = chats::list_chats(state.db(), Some(workspace_id), archived).await?;
    let ids: Vec<Uuid> = chats.iter().map(|chat| chat.id).collect();
    let (writing, awaiting_answer, rows) = tokio::try_join!(
        activity::writing(state.db(), workspace_id),
        activity::awaiting_answer(state.db(), workspace_id),
        chat_pull_requests::for_chats(state.db(), &ids),
    )?;
    let mut pulls: HashMap<Uuid, Vec<PullIndicator>> = HashMap::new();
    for (chat_id, pull) in signals(state, workspace_id, rows).await {
        pulls.entry(chat_id).or_default().push(pull);
    }

    Ok(ids
        .into_iter()
        .map(|chat_id| {
            let mut mine = pulls.remove(&chat_id).unwrap_or_default();
            mine.sort_by_key(PullIndicator::rank);
            Indicator {
                chat_id,
                activity: Activity::read(
                    writing.contains(&chat_id),
                    ApprovalPolicy::chat_waiting(chat_id),
                    awaiting_answer.contains(&chat_id),
                ),
                pull_requests: mine.len(),
                pull_request: mine.into_iter().next(),
            }
        })
        .collect())
}

/// What GitHub last said of each of the workspace's recorded pull requests,
/// asking it again only for the ones with no reading fresh enough for a list
/// and only while the budget for this read lasts.
async fn signals(
    state: &AppState,
    workspace_id: Uuid,
    rows: Vec<PullRequestRow>,
) -> Vec<(Uuid, PullIndicator)> {
    let deadline = Instant::now().checked_add(REFRESH_BUDGET);
    stream::iter(rows)
        .map(|row| signal(state, workspace_id, row, deadline))
        .buffered(CONCURRENT_REFRESHES)
        .collect()
        .await
}

async fn signal(
    state: &AppState,
    workspace_id: Uuid,
    row: PullRequestRow,
    deadline: Option<Instant>,
) -> (Uuid, PullIndicator) {
    let chat_id = row.chat_id;
    if let Some(status) = state.pull_request_statuses().within(row.id, LIST_FRESHNESS) {
        return (chat_id, PullIndicator::new(&row, Some(status)));
    }
    if !deadline.is_some_and(|deadline| Instant::now() < deadline) {
        return (chat_id, PullIndicator::new(&row, None));
    }
    let status = match pull_request::track(state, workspace_id, row.clone()).await {
        Ok(tracked) => tracked.status,
        Err(error) => {
            tracing::warn!(pull_request = %row.id, %error, "could not read a chat's pull request for its row");
            None
        }
    };
    (chat_id, PullIndicator::new(&row, status))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::services::chat::pull_request::{AutoMerge, Mergeable, Threads};

    fn row() -> PullRequestRow {
        PullRequestRow {
            id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
            provider: chat_pull_requests::GITHUB.to_string(),
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            number: 7,
            url: "https://github.com/acme/widgets/pull/7".to_string(),
            title: Some("Add widgets".to_string()),
            source_id: None,
            dismissed_at: None,
            created_at: Utc::now(),
            auto_fix: false,
            auto_fix_run_id: None,
            auto_fix_by: None,
            auto_fix_head: None,
            auto_fixing: false,
        }
    }

    fn status(
        state: State,
        checks: Checks,
        mergeable: Mergeable,
        open_threads: Option<u32>,
    ) -> Status {
        Status {
            title: "Add widgets".to_string(),
            state,
            draft: false,
            additions: 4,
            deletions: 1,
            changed_files: 2,
            checks,
            mergeable,
            auto_merge: AutoMerge::unavailable(AutoMerge::UNREAD),
            draft_unavailable: None,
            threads: open_threads.map(|open| Threads { open, resolved: 0 }),
            observed_at: Utc::now(),
        }
    }

    fn indicator(status: Option<Status>) -> PullIndicator {
        PullIndicator::new(&row(), status)
    }

    #[test]
    fn an_approval_outranks_a_live_turn_and_a_question_waits_for_one_to_end() {
        assert_eq!(
            Activity::read(true, true, false),
            Activity::AwaitingApproval,
            "a turn parked on an approval read as writing"
        );
        assert_eq!(Activity::read(true, false, true), Activity::Writing);
        assert_eq!(Activity::read(false, false, true), Activity::AwaitingAnswer);
        assert_eq!(Activity::read(false, false, false), Activity::Idle);
    }

    #[test]
    fn only_awaiting_states_count_as_waiting_for_a_person() {
        assert!(Activity::AwaitingApproval.waiting());
        assert!(Activity::AwaitingAnswer.waiting());
        assert!(!Activity::Writing.waiting());
        assert!(!Activity::Idle.waiting());
    }

    #[test]
    fn readiness_is_carried_from_the_status_and_never_from_an_absent_one() {
        let ready = indicator(Some(status(
            State::Open,
            Checks::Passing,
            Mergeable::Clean,
            Some(0),
        )));
        assert!(ready.ready);

        let commented = indicator(Some(status(
            State::Open,
            Checks::Passing,
            Mergeable::Clean,
            Some(2),
        )));
        assert!(!commented.ready, "an open review thread read as ready");

        assert!(!indicator(None).ready, "an unread status read as ready");
    }

    #[test]
    fn a_failing_check_wants_attention_before_anything_settled() {
        let mut ranked = [
            (
                "merged",
                indicator(Some(status(
                    State::Merged,
                    Checks::None,
                    Mergeable::Unknown,
                    None,
                ))),
            ),
            (
                "ready",
                indicator(Some(status(
                    State::Open,
                    Checks::Passing,
                    Mergeable::Clean,
                    Some(0),
                ))),
            ),
            ("unread", indicator(None)),
            (
                "failing",
                indicator(Some(status(
                    State::Open,
                    Checks::Failing,
                    Mergeable::Unstable,
                    Some(0),
                ))),
            ),
            (
                "running",
                indicator(Some(status(
                    State::Open,
                    Checks::Pending,
                    Mergeable::Unstable,
                    Some(0),
                ))),
            ),
            (
                "blocked",
                indicator(Some(status(
                    State::Open,
                    Checks::Passing,
                    Mergeable::Conflicted,
                    Some(0),
                ))),
            ),
        ];
        ranked.sort_by_key(|(_, pull)| pull.rank());

        assert_eq!(
            ranked.map(|(name, _)| name),
            ["failing", "running", "blocked", "ready", "unread", "merged"]
        );
    }

    #[test]
    fn a_draft_is_shown_before_a_settled_pull_request_and_after_an_unread_one() {
        let draft = PullIndicator::new(
            &row(),
            Some(Status {
                draft: true,
                ..status(State::Open, Checks::Passing, Mergeable::Draft, Some(0))
            }),
        );
        let closed = indicator(Some(status(
            State::Closed,
            Checks::None,
            Mergeable::Unknown,
            None,
        )));

        assert!(draft.rank() > indicator(None).rank());
        assert!(draft.rank() < closed.rank());
    }
}
