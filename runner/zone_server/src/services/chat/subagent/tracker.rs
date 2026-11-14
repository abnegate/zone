//! Keeping a chat's subagents as one of its turns reports them.

use std::sync::Arc;

use abnegate_agent_cli::Subagent;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use super::Announcer;
use super::keeper::Keeper;
use crate::state::AppState;

/// Stores what one chat turn's agent says about its subagents, and announces
/// each stored change to the chat's readers.
///
/// As the turn starts, the tracker stops what earlier turns left running,
/// whether or not its own agent ever names a subagent. An agent can report
/// many steps in one burst, so the updates already waiting are folded
/// together: each subagent they touch is written and announced once per burst,
/// with only the steps that burst changed.
#[derive(Debug)]
pub struct Tracker {
    updates: UnboundedSender<Subagent>,
}

impl Tracker {
    /// Keeps turn `message_id`'s subagents. The turn's tracker is done once
    /// every clone of the observer holding it is gone.
    pub fn new(
        state: AppState,
        chat_id: Uuid,
        message_id: Uuid,
        announcer: Arc<dyn Announcer>,
    ) -> Self {
        Self::kept(Keeper::new(state, chat_id, message_id, announcer))
    }

    fn kept(keeper: Keeper) -> Self {
        Self {
            updates: keeper.start(),
        }
    }

    pub fn track(&self, subagent: &Subagent) {
        let _ = self.updates.send(subagent.clone());
    }

    /// A tracker whose updates go to the returned receiver instead.
    #[cfg(test)]
    pub(crate) fn listening() -> (Self, tokio::sync::mpsc::UnboundedReceiver<Subagent>) {
        let (updates, received) = tokio::sync::mpsc::unbounded_channel();
        (Self { updates }, received)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use abnegate_agent_cli::{StepStatus, SubagentStatus, SubagentStep};
    use sqlx::PgPool;
    use tokio::sync::Notify;
    use tokio::sync::mpsc::{self, UnboundedReceiver};

    use super::*;
    use crate::db::chat_subagents::{self, SubagentChange};
    use crate::db::chats::create_chat;
    use crate::db::context::RUNNING as TURN_RUNNING;
    use crate::services::chat::subagent::ChatSubagent;
    use crate::services::chat::subagent::keeper::FINISH_ATTEMPTS;

    const PATIENCE: Duration = Duration::from_secs(10);

    type Announced = UnboundedReceiver<(ChatSubagent, Vec<SubagentStep>)>;

    struct Turn {
        state: AppState,
        chat_id: Uuid,
        message_id: Uuid,
    }

    impl Turn {
        async fn new() -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("isolated test database"),
            )
            .await
            .expect("the test database accepts connections");
            let chat_id = chat(&pool).await;
            let message_id = running_turn(&pool, chat_id).await;
            Self {
                state: AppState::new(crate::state::test_config(), pool, None),
                chat_id,
                message_id,
            }
        }

        fn track(&self) -> (Tracker, Announced) {
            let (announce, announced) = mpsc::unbounded_channel();
            let tracker = Tracker::new(
                self.state.clone(),
                self.chat_id,
                self.message_id,
                Arc::new(announce),
            );
            (tracker, announced)
        }

        /// A tracker that writes nothing until `hold` is notified, so every
        /// update a test sends before that arrives as one burst.
        fn held(&self) -> (Tracker, Announced, Arc<Notify>) {
            let (tracker, announced, hold, _) = self.failing(0);
            (tracker, announced, hold)
        }

        /// A held tracker whose first `failures` writes fail as an outage
        /// would, and the count of those still to fail.
        fn failing(&self, failures: usize) -> (Tracker, Announced, Arc<Notify>, Arc<AtomicUsize>) {
            self.deadlocking(failures, 0)
        }

        /// A failing tracker whose first `deadlocks` final transactions are
        /// aborted as deadlock victims just before their stop.
        fn deadlocking(
            &self,
            failures: usize,
            deadlocks: usize,
        ) -> (Tracker, Announced, Arc<Notify>, Arc<AtomicUsize>) {
            let (announce, announced) = mpsc::unbounded_channel();
            let mut keeper = Keeper::new(
                self.state.clone(),
                self.chat_id,
                self.message_id,
                Arc::new(announce),
            );
            let hold = Arc::new(Notify::new());
            keeper.hold = Some(hold.clone());
            keeper.outage.store(failures, Ordering::SeqCst);
            keeper.deadlocks.store(deadlocks, Ordering::SeqCst);
            let outage = keeper.outage.clone();
            (Tracker::kept(keeper), announced, hold, outage)
        }

        /// What deleting the turn's message leaves of it: nothing.
        async fn delete(&self) {
            chat_subagents::remove(self.state.db(), self.chat_id, self.message_id)
                .await
                .expect("the turn's subagents removed");
            sqlx::query("DELETE FROM chat_turns WHERE chat_id = $1 AND id = $2")
                .bind(self.chat_id)
                .bind(self.message_id)
                .execute(self.state.db())
                .await
                .expect("the turn deleted");
        }

        async fn running(&self, chat_id: Uuid, external_id: &str, message_id: Option<Uuid>) {
            chat_subagents::record(
                self.state.db(),
                &SubagentChange {
                    chat_id,
                    message_id,
                    external_id,
                    ..SubagentChange::default()
                },
            )
            .await
            .expect("a running subagent");
        }

        async fn status(&self, chat_id: Uuid, external_id: &str) -> String {
            chat_subagents::list(self.state.db(), chat_id)
                .await
                .expect("the chat's subagents")
                .into_iter()
                .find(|row| row.external_id == external_id)
                .expect("the subagent")
                .status
        }

        async fn steps(&self, subagent: &ChatSubagent) -> Vec<SubagentStep> {
            chat_subagents::steps(self.state.db(), self.chat_id, subagent.id)
                .await
                .expect("the subagent's steps")
                .expect("the subagent")
        }
    }

    /// A running turn of `chat_id`, as the chat's store begins it.
    async fn running_turn(pool: &PgPool, chat_id: Uuid) -> Uuid {
        let message = Uuid::new_v4();
        let turn = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO messages (id, chat_id, role, content) VALUES ($1, $2, 'user', 'Go')",
        )
        .bind(message)
        .bind(chat_id)
        .execute(pool)
        .await
        .expect("a user message");
        sqlx::query(
            "INSERT INTO chat_turns (id, chat_id, user_message_id, fence, status) VALUES ($1, $2, $3, 1, $4)",
        )
        .bind(turn)
        .bind(chat_id)
        .bind(message)
        .bind(TURN_RUNNING)
        .execute(pool)
        .await
        .expect("a turn");
        turn
    }

    async fn chat(pool: &PgPool) -> Uuid {
        create_chat(pool, None, "Subagents", "sonnet", false, true)
            .await
            .expect("a chat")
            .id
    }

    async fn next(announced: &mut Announced) -> (ChatSubagent, Vec<SubagentStep>) {
        tokio::time::timeout(PATIENCE, announced.recv())
            .await
            .expect("an announcement in time")
            .expect("an announcement")
    }

    /// Every announcement still to come, once the tracker has finished.
    async fn rest(mut announced: Announced) -> Vec<(ChatSubagent, Vec<SubagentStep>)> {
        let mut rest = Vec::new();
        loop {
            match tokio::time::timeout(PATIENCE, announced.recv()).await {
                Ok(Some(announcement)) => rest.push(announcement),
                Ok(None) => return rest,
                Err(_) => panic!("the tracker never finished: {rest:?}"),
            }
        }
    }

    fn step(id: &str, status: StepStatus) -> SubagentStep {
        SubagentStep::new(id, "Bash", status).with_subject("cargo test")
    }

    fn stepped(id: &str, step: SubagentStep) -> Subagent {
        Subagent::new(id).with_step(step)
    }

    fn finished(id: &str) -> Subagent {
        Subagent::new(id).with_status(SubagentStatus::Completed)
    }

    #[tokio::test]
    async fn updates_in_turn_merge_into_one_subagent_and_announce_only_the_steps_they_changed() {
        let turn = Turn::new().await;
        let (tracker, mut announced) = turn.track();

        tracker.track(
            &Subagent::new("toolu_01Agent")
                .with_description("Ask for a review")
                .with_kind("general-purpose")
                .with_background(true)
                .with_step(step("toolu_01Step", StepStatus::Running)),
        );
        let first = next(&mut announced).await;
        tracker.track(&stepped(
            "toolu_01Agent",
            step("toolu_01Step", StepStatus::Succeeded),
        ));
        let second = next(&mut announced).await;
        tracker.track(
            &Subagent::new("toolu_01Agent")
                .with_tool_uses(2)
                .with_last_tool("Read")
                .with_step(step("toolu_02Step", StepStatus::Running)),
        );
        let third = next(&mut announced).await;
        tracker.track(&finished("toolu_01Agent").with_summary("Looks good"));
        let (last, settled) = next(&mut announced).await;
        drop(tracker);

        assert_eq!(first.1, [step("toolu_01Step", StepStatus::Running)]);
        assert_eq!(second.1, [step("toolu_01Step", StepStatus::Succeeded)]);
        assert_eq!(third.1, [step("toolu_02Step", StepStatus::Running)]);
        assert_eq!(
            settled,
            [step("toolu_02Step", StepStatus::Failed)],
            "a step still running when its subagent finished is settled with it"
        );
        assert_eq!(
            [
                first.0.step_count,
                second.0.step_count,
                third.0.step_count,
                last.step_count
            ],
            [1, 1, 2, 2]
        );
        let revisions = [
            first.0.revision,
            second.0.revision,
            third.0.revision,
            last.revision,
        ];
        assert!(
            revisions.windows(2).all(|pair| pair[0] < pair[1]),
            "{revisions:?}"
        );
        assert_eq!(last.message_id, Some(turn.message_id));
        assert_eq!(last.description, "Ask for a review");
        assert_eq!(last.kind.as_deref(), Some("general-purpose"));
        assert!(last.background);
        assert_eq!(last.tool_uses, Some(2));
        assert_eq!(last.last_tool.as_deref(), Some("Read"));
        assert_eq!(last.summary.as_deref(), Some("Looks good"));
        assert_eq!(last.status, SubagentStatus::Completed);
        assert_eq!(
            turn.steps(&last).await,
            [
                step("toolu_01Step", StepStatus::Succeeded),
                step("toolu_02Step", StepStatus::Failed),
            ]
        );
        assert_eq!(
            rest(announced).await,
            [],
            "a finished subagent is not swept"
        );
        let stored = chat_subagents::list(turn.state.db(), turn.chat_id)
            .await
            .expect("the chat's subagents");
        assert_eq!(stored.len(), 1, "{stored:?}");
        assert_eq!(ChatSubagent::from(stored[0].clone()), last);
    }

    #[tokio::test]
    async fn a_burst_of_steps_is_written_and_announced_once() {
        let turn = Turn::new().await;
        let (tracker, announced, hold) = turn.held();
        let steps: Vec<SubagentStep> = (0..20)
            .map(|index| step(&format!("toolu_{index:02}"), StepStatus::Succeeded))
            .collect();

        for step in &steps {
            tracker.track(&stepped("toolu_01Agent", step.clone()));
        }
        tracker.track(&finished("toolu_01Agent"));
        drop(tracker);
        hold.notify_one();

        let announcements = rest(announced).await;
        assert_eq!(announcements.len(), 1, "{announcements:?}");
        let (subagent, changed) = &announcements[0];
        assert_eq!(changed, &steps);
        assert_eq!(subagent.step_count, 20);
        assert_eq!(subagent.status, SubagentStatus::Completed);
        assert_eq!(turn.steps(subagent).await, steps);
    }

    #[tokio::test]
    async fn a_subagents_duration_is_stored_in_milliseconds() {
        const RUN: Duration = Duration::from_micros(61_000_999);
        const STORED: i64 = 61_000;
        let turn = Turn::new().await;
        let (tracker, mut announced) = turn.track();

        tracker.track(&Subagent::new("toolu_01Agent").with_duration(RUN));
        let (running, _) = next(&mut announced).await;
        tracker.track(&finished("toolu_01Agent").with_duration(Duration::MAX));
        let (ended, _) = next(&mut announced).await;
        drop(tracker);

        assert_eq!(running.duration_ms, Some(STORED));
        assert_eq!(
            ended.duration_ms,
            Some(i64::MAX),
            "a duration past what the column holds is stored as its most"
        );
        let stored = chat_subagents::list(turn.state.db(), turn.chat_id)
            .await
            .expect("the chat's subagents");
        assert_eq!(
            stored.iter().map(|row| row.duration_ms).collect::<Vec<_>>(),
            [Some(i64::MAX)]
        );
        assert_eq!(rest(announced).await, []);
    }

    #[tokio::test]
    async fn subagents_reported_together_are_announced_once_each() {
        let turn = Turn::new().await;
        let (tracker, announced, hold) = turn.held();

        tracker.track(&stepped("thread-1", step("call-1", StepStatus::Running)));
        tracker.track(&stepped("thread-2", step("call-2", StepStatus::Running)));
        tracker.track(&stepped("thread-1", step("call-1", StepStatus::Succeeded)));
        tracker.track(&finished("thread-2"));
        tracker.track(&finished("thread-1"));
        drop(tracker);
        hold.notify_one();

        let announcements = rest(announced).await;
        let named: Vec<&str> = announcements
            .iter()
            .map(|(subagent, _)| subagent.external_id.as_str())
            .collect();
        assert_eq!(
            named,
            ["thread-1", "thread-2"],
            "in the order first heard of"
        );
        assert_eq!(announcements[0].1, [step("call-1", StepStatus::Succeeded)]);
        assert_eq!(
            announcements[1].1,
            [step("call-2", StepStatus::Failed)],
            "a step still running when its subagent finished never runs"
        );
    }

    #[tokio::test]
    async fn a_turn_stops_what_earlier_turns_left_running_before_it_writes() {
        let turn = Turn::new().await;
        let elsewhere = chat(turn.state.db()).await;
        turn.running(turn.chat_id, "earlier", Some(Uuid::new_v4()))
            .await;
        turn.running(turn.chat_id, "unowned", None).await;
        turn.running(elsewhere, "foreign", Some(Uuid::new_v4()))
            .await;
        let earlier = chat_subagents::list(turn.state.db(), turn.chat_id)
            .await
            .expect("the chat's subagents")
            .into_iter()
            .find(|row| row.external_id == "earlier")
            .expect("the earlier subagent");
        chat_subagents::record_steps(
            turn.state.db(),
            earlier.id,
            &[
                step("toolu_done", StepStatus::Succeeded),
                step("toolu_open", StepStatus::Running),
            ],
        )
        .await
        .expect("the earlier subagent's steps");

        let (tracker, announced) = turn.track();
        tracker.track(&finished("mine"));
        drop(tracker);

        let announcements = rest(announced).await;
        let heard: Vec<(&str, SubagentStatus, &[SubagentStep])> = announcements
            .iter()
            .map(|(subagent, steps)| {
                (
                    subagent.external_id.as_str(),
                    subagent.status,
                    steps.as_slice(),
                )
            })
            .collect();
        assert_eq!(
            heard,
            [
                (
                    "earlier",
                    SubagentStatus::Stopped,
                    [step("toolu_open", StepStatus::Failed)].as_slice()
                ),
                ("unowned", SubagentStatus::Stopped, [].as_slice()),
                ("mine", SubagentStatus::Completed, [].as_slice()),
            ]
        );
        assert_eq!(
            turn.steps(&announcements[0].0).await,
            [
                step("toolu_done", StepStatus::Succeeded),
                step("toolu_open", StepStatus::Failed),
            ]
        );
        assert_eq!(
            turn.status(elsewhere, "foreign").await,
            SubagentStatus::Running.as_str(),
            "another chat's subagents are its own turns' to stop"
        );
    }

    #[tokio::test]
    async fn a_turn_that_ends_stops_the_subagents_it_left_running_and_fails_their_steps() {
        let turn = Turn::new().await;
        let (tracker, mut announced) = turn.track();

        tracker.track(&stepped(
            "toolu_01Agent",
            step("toolu_x", StepStatus::Running),
        ));
        let (started, _) = next(&mut announced).await;
        drop(tracker);

        assert_eq!(started.status, SubagentStatus::Running);
        let announcements = rest(announced).await;
        assert_eq!(announcements.len(), 1, "{announcements:?}");
        let (stopped, failed) = &announcements[0];
        assert_eq!(stopped.id, started.id);
        assert_eq!(stopped.status, SubagentStatus::Stopped);
        assert!(stopped.finished_at.is_some());
        assert!(stopped.revision > started.revision);
        assert_eq!(failed, &[step("toolu_x", StepStatus::Failed)]);
        assert_eq!(
            turn.steps(stopped).await,
            [step("toolu_x", StepStatus::Failed)]
        );
    }

    #[tokio::test]
    async fn a_turn_that_hears_of_no_subagent_still_stops_what_earlier_turns_left_running() {
        let turn = Turn::new().await;
        turn.running(turn.chat_id, "earlier", Some(Uuid::new_v4()))
            .await;

        let (tracker, announced) = turn.track();
        drop(tracker);

        let heard: Vec<(String, SubagentStatus)> = rest(announced)
            .await
            .into_iter()
            .map(|(subagent, _)| (subagent.external_id, subagent.status))
            .collect();
        assert_eq!(heard, [("earlier".to_string(), SubagentStatus::Stopped)]);
        assert_eq!(
            turn.status(turn.chat_id, "earlier").await,
            SubagentStatus::Stopped.as_str()
        );
    }

    #[tokio::test]
    async fn a_subagent_that_finishes_fails_the_steps_it_never_reported_on() {
        let turn = Turn::new().await;
        let (tracker, mut announced) = turn.track();

        tracker.track(&stepped(
            "toolu_01Agent",
            step("toolu_lost", StepStatus::Running),
        ));
        next(&mut announced).await;
        tracker.track(&finished("toolu_01Agent"));
        let (finished, steps) = next(&mut announced).await;
        drop(tracker);

        assert_eq!(finished.status, SubagentStatus::Completed);
        assert_eq!(steps, [step("toolu_lost", StepStatus::Failed)]);
        assert_eq!(
            turn.steps(&finished).await,
            [step("toolu_lost", StepStatus::Failed)]
        );
        assert_eq!(rest(announced).await, []);
    }

    #[tokio::test]
    async fn a_subagent_its_turns_settling_stopped_is_announced_stopped() {
        let turn = Turn::new().await;
        let (tracker, mut announced) = turn.track();
        tracker.track(&stepped(
            "toolu_01Agent",
            step("toolu_open", StepStatus::Running),
        ));
        let (started, _) = next(&mut announced).await;

        chat_subagents::stop(turn.state.db(), turn.chat_id, turn.message_id)
            .await
            .expect("the turn's interruption");
        drop(tracker);

        let announcements = rest(announced).await;
        assert_eq!(announcements.len(), 1, "{announcements:?}");
        let (stopped, failed) = &announcements[0];
        assert_eq!(stopped.id, started.id);
        assert_eq!(stopped.status, SubagentStatus::Stopped);
        assert!(stopped.revision > started.revision);
        assert_eq!(failed, &[step("toolu_open", StepStatus::Failed)]);
    }

    #[tokio::test]
    async fn an_update_an_outage_kept_from_the_database_is_written_once_it_can_be() {
        let turn = Turn::new().await;
        let (tracker, announced, hold, _) = turn.failing(1);

        tracker.track(&finished("toolu_01Agent").with_summary("Looks good"));
        drop(tracker);
        hold.notify_one();

        let announcements = rest(announced).await;
        assert_eq!(announcements.len(), 1, "{announcements:?}");
        let (subagent, _) = &announcements[0];
        assert_eq!(subagent.status, SubagentStatus::Completed);
        assert_eq!(subagent.summary.as_deref(), Some("Looks good"));
        assert_eq!(
            turn.status(turn.chat_id, "toolu_01Agent").await,
            SubagentStatus::Completed.as_str()
        );
    }

    #[tokio::test]
    async fn updates_kept_from_the_database_go_ahead_of_the_next_burst() {
        let turn = Turn::new().await;
        let (tracker, mut announced, hold, outage) = turn.failing(1);
        tracker.track(
            &Subagent::new("toolu_01Agent")
                .with_description("Survey")
                .with_step(step("toolu_first", StepStatus::Running)),
        );
        hold.notify_one();
        tokio::time::timeout(PATIENCE, async {
            while outage.load(Ordering::SeqCst) > 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the outage over in time");

        tracker.track(&stepped(
            "toolu_01Agent",
            step("toolu_second", StepStatus::Running),
        ));
        let (subagent, steps) = next(&mut announced).await;

        let both = [
            step("toolu_first", StepStatus::Running),
            step("toolu_second", StepStatus::Running),
        ];
        assert_eq!(steps, both);
        assert_eq!(subagent.description, "Survey");
        assert_eq!(subagent.step_count, 2);
        assert_eq!(turn.steps(&subagent).await, both);
        drop(tracker);
        rest(announced).await;
    }

    /// A turn whose subagent `toolu_01Agent` was announced running with step
    /// `toolu_open` open, and then finished while the first `failures`
    /// transactions after that fail as an outage would, and the first
    /// `deadlocks` of its final transactions deadlock.
    async fn finished_in_an_outage(
        turn: &Turn,
        failures: usize,
        deadlocks: usize,
    ) -> (ChatSubagent, Vec<(ChatSubagent, Vec<SubagentStep>)>) {
        let (tracker, mut announced, hold, outage) = turn.deadlocking(0, deadlocks);
        hold.notify_one();
        tracker.track(&stepped(
            "toolu_01Agent",
            step("toolu_open", StepStatus::Running),
        ));
        let (started, _) = next(&mut announced).await;

        outage.store(failures, Ordering::SeqCst);
        tracker.track(&finished("toolu_01Agent").with_summary("Looks good"));
        drop(tracker);

        let announcements = rest(announced).await;
        assert_eq!(outage.load(Ordering::SeqCst), 0, "every failure was met");
        (started, announcements)
    }

    #[tokio::test]
    async fn an_outcome_whose_last_retry_failed_is_written_with_the_turns_final_stop() {
        let turn = Turn::new().await;

        let (started, announcements) = finished_in_an_outage(&turn, 2, 0).await;

        assert_eq!(announcements.len(), 1, "{announcements:?}");
        let (subagent, steps) = &announcements[0];
        assert_eq!(subagent.id, started.id);
        assert_eq!(
            subagent.status,
            SubagentStatus::Completed,
            "the turn's stop never hides the outcome it wrote first"
        );
        assert_eq!(subagent.summary.as_deref(), Some("Looks good"));
        assert_eq!(steps, &[step("toolu_open", StepStatus::Failed)]);
        assert_eq!(
            turn.status(turn.chat_id, "toolu_01Agent").await,
            SubagentStatus::Completed.as_str()
        );
        assert_eq!(
            turn.steps(subagent).await,
            [step("toolu_open", StepStatus::Failed)]
        );
    }

    #[tokio::test]
    async fn a_final_stop_that_fails_leaves_the_outcome_and_the_stop_to_the_sweep() {
        let turn = Turn::new().await;

        let every_try = 2 + usize::try_from(FINISH_ATTEMPTS).expect("a few tries");
        let (started, announcements) = finished_in_an_outage(&turn, every_try, 0).await;

        assert_eq!(announcements, [], "nothing was written to announce");
        assert_eq!(
            turn.status(turn.chat_id, "toolu_01Agent").await,
            SubagentStatus::Running.as_str(),
            "neither the outcome nor the stop landed"
        );
        assert_eq!(
            turn.steps(&started).await,
            [step("toolu_open", StepStatus::Running)]
        );
    }

    #[tokio::test]
    async fn an_outcome_whose_final_stop_deadlocked_is_written_when_it_is_tried_again() {
        let turn = Turn::new().await;

        let (started, announcements) = finished_in_an_outage(&turn, 2, 1).await;

        assert_eq!(announcements.len(), 1, "{announcements:?}");
        let (subagent, steps) = &announcements[0];
        assert_eq!(subagent.id, started.id);
        assert_eq!(
            subagent.status,
            SubagentStatus::Completed,
            "the deadlock victim's retry still writes the outcome"
        );
        assert_eq!(subagent.summary.as_deref(), Some("Looks good"));
        assert_eq!(steps, &[step("toolu_open", StepStatus::Failed)]);
        assert_eq!(
            turn.status(turn.chat_id, "toolu_01Agent").await,
            SubagentStatus::Completed.as_str()
        );
    }

    #[tokio::test]
    async fn an_outcome_the_database_refuses_drops_alone_from_the_turns_final_stop() {
        let turn = Turn::new().await;
        turn.running(turn.chat_id, "toolu_refused", Some(turn.message_id))
            .await;
        let (tracker, announced, hold, outage) = turn.failing(4);

        tracker.track(&finished("toolu_kept").with_summary("Looks good"));
        tracker.track(&finished("toolu_refused").with_summary("Nul\0byte"));
        drop(tracker);
        hold.notify_one();

        let heard: Vec<(String, SubagentStatus, Option<String>)> = rest(announced)
            .await
            .into_iter()
            .map(|(subagent, _)| (subagent.external_id, subagent.status, subagent.summary))
            .collect();
        assert_eq!(outage.load(Ordering::SeqCst), 0, "every failure was met");
        assert_eq!(
            heard,
            [
                (
                    "toolu_kept".to_string(),
                    SubagentStatus::Completed,
                    Some("Looks good".to_string())
                ),
                ("toolu_refused".to_string(), SubagentStatus::Stopped, None),
            ]
        );
        assert_eq!(
            turn.status(turn.chat_id, "toolu_kept").await,
            SubagentStatus::Completed.as_str()
        );
        assert_eq!(
            turn.status(turn.chat_id, "toolu_refused").await,
            SubagentStatus::Stopped.as_str(),
            "the turn's stop lands without the update it could not write"
        );
    }

    #[tokio::test]
    async fn an_update_for_a_deleted_turn_brings_none_of_its_subagents_back() {
        let turn = Turn::new().await;
        let (tracker, mut announced) = turn.track();
        tracker.track(&stepped(
            "toolu_01Agent",
            step("toolu_open", StepStatus::Running),
        ));
        next(&mut announced).await;

        turn.delete().await;
        tracker.track(&finished("toolu_01Agent"));
        drop(tracker);

        assert_eq!(rest(announced).await, []);
        assert_eq!(
            chat_subagents::list(turn.state.db(), turn.chat_id)
                .await
                .expect("the chat's subagents"),
            []
        );
    }
}
