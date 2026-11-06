//! Keeping a chat's subagents as one of its turns reports them.

use std::sync::Arc;

use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;
use zone_core::llm::Subagent;

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
    use std::time::Duration;

    use sqlx::PgPool;
    use tokio::sync::Notify;
    use tokio::sync::mpsc::{self, UnboundedReceiver};
    use zone_core::llm::{StepStatus, SubagentStatus, SubagentStep};

    use super::*;
    use crate::db::chat_subagents::{self, SubagentChange};
    use crate::db::chats::create_chat;
    use crate::services::chat::subagent::ChatSubagent;

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
            Self {
                state: AppState::new(crate::state::test_config(), pool, None),
                chat_id,
                message_id: Uuid::new_v4(),
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
            let (announce, announced) = mpsc::unbounded_channel();
            let mut keeper = Keeper::new(
                self.state.clone(),
                self.chat_id,
                self.message_id,
                Arc::new(announce),
            );
            let hold = Arc::new(Notify::new());
            keeper.hold = Some(hold.clone());
            (Tracker::kept(keeper), announced, hold)
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
        SubagentStep {
            id: id.to_string(),
            name: "Bash".to_string(),
            subject: Some("cargo test".to_string()),
            status,
        }
    }

    fn stepped(id: &str, step: SubagentStep) -> Subagent {
        Subagent {
            step: Some(step),
            ..Subagent::new(id)
        }
    }

    fn finished(id: &str) -> Subagent {
        Subagent {
            status: Some(SubagentStatus::Completed),
            ..Subagent::new(id)
        }
    }

    #[tokio::test]
    async fn updates_in_turn_merge_into_one_subagent_and_announce_only_the_steps_they_changed() {
        let turn = Turn::new().await;
        let (tracker, mut announced) = turn.track();

        tracker.track(&Subagent {
            description: Some("Ask for a review".to_string()),
            kind: Some("general-purpose".to_string()),
            background: Some(true),
            step: Some(step("toolu_01Step", StepStatus::Running)),
            ..Subagent::new("toolu_01Agent")
        });
        let first = next(&mut announced).await;
        tracker.track(&stepped(
            "toolu_01Agent",
            step("toolu_01Step", StepStatus::Succeeded),
        ));
        let second = next(&mut announced).await;
        tracker.track(&Subagent {
            tool_uses: Some(2),
            last_tool: Some("Read".to_string()),
            step: Some(step("toolu_02Step", StepStatus::Running)),
            ..Subagent::new("toolu_01Agent")
        });
        let third = next(&mut announced).await;
        tracker.track(&Subagent {
            summary: Some("Looks good".to_string()),
            ..finished("toolu_01Agent")
        });
        let (last, unstepped) = next(&mut announced).await;
        drop(tracker);

        assert_eq!(first.1, [step("toolu_01Step", StepStatus::Running)]);
        assert_eq!(second.1, [step("toolu_01Step", StepStatus::Succeeded)]);
        assert_eq!(third.1, [step("toolu_02Step", StepStatus::Running)]);
        assert_eq!(unstepped, []);
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
                step("toolu_02Step", StepStatus::Running),
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
}
