//! Keeping a chat's subagents as one of its turns reports them.

use std::collections::HashMap;
use std::fmt;

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use uuid::Uuid;
use zone_core::llm::{Subagent, SubagentStep};

use super::ChatSubagent;
use crate::db::DbResult;
use crate::db::chat_subagents::{self, SubagentChange, SubagentRow};
use crate::state::AppState;

/// Stores what one chat turn's agent says about its subagents, and announces
/// each stored change to the chat's readers.
///
/// An agent can report many steps in one burst, so the updates already
/// waiting are folded together: each subagent they touch is written and
/// announced once per burst rather than once per update.
pub struct Tracker {
    updates: UnboundedSender<Subagent>,
}

impl Tracker {
    /// Starts keeping turn `message_id`'s subagents. The turn's tracker is
    /// done once every clone of the observer holding it is gone.
    pub fn spawn(
        state: AppState,
        chat_id: Uuid,
        message_id: Uuid,
        announce: UnboundedSender<ChatSubagent>,
    ) -> Self {
        let (updates, received) = mpsc::unbounded_channel();
        let keeper = Keeper {
            state,
            chat_id,
            message_id,
            announce,
            steps: HashMap::new(),
        };
        tokio::spawn(keeper.run(received));
        Self { updates }
    }

    pub fn track(&self, subagent: &Subagent) {
        let _ = self.updates.send(subagent.clone());
    }

    #[cfg(test)]
    pub(crate) fn listening() -> (Self, UnboundedReceiver<Subagent>) {
        let (updates, received) = mpsc::unbounded_channel();
        (Self { updates }, received)
    }
}

impl fmt::Debug for Tracker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Tracker").finish_non_exhaustive()
    }
}

struct Keeper {
    state: AppState,
    chat_id: Uuid,
    message_id: Uuid,
    announce: UnboundedSender<ChatSubagent>,
    /// Every step each subagent has reported this turn, since a stored list
    /// is replaced whole rather than appended to.
    steps: HashMap<String, Vec<SubagentStep>>,
}

impl Keeper {
    async fn run(mut self, mut updates: UnboundedReceiver<Subagent>) {
        let others =
            chat_subagents::stop_others(self.state.db(), self.chat_id, self.message_id).await;
        self.settled(
            others,
            "could not stop the subagents an earlier turn left running",
        );

        let mut heard = false;
        while let Some(first) = updates.recv().await {
            heard = true;
            let mut batch = Batch::default();
            batch.add(first, &mut self.steps);
            while let Ok(next) = updates.try_recv() {
                batch.add(next, &mut self.steps);
            }
            for pending in batch.pending {
                self.record(pending).await;
            }
        }

        if heard {
            let stopped =
                chat_subagents::stop(self.state.db(), self.chat_id, self.message_id).await;
            self.settled(stopped, "could not stop the subagents a turn left running");
        }
    }

    async fn record(&self, pending: Pending) {
        let update = pending.update;
        let steps = match pending.stepped {
            true => self.steps.get(&update.id).map(Vec::as_slice),
            false => None,
        };
        let change = SubagentChange {
            chat_id: self.chat_id,
            message_id: Some(self.message_id),
            external_id: &update.id,
            description: update.description.as_deref(),
            kind: update.kind.as_deref(),
            model: update.model.as_deref(),
            background: update.background,
            status: update.status,
            tool_uses: update.tool_uses.map(stored),
            duration_ms: update.duration_ms.map(stored),
            tokens: update.tokens.map(stored),
            last_tool: update.last_tool.as_deref(),
            summary: update.summary.as_deref(),
            error: update.error.as_deref(),
            steps,
        };
        match chat_subagents::record(self.state.db(), &change).await {
            Ok(row) => self.publish(row),
            Err(error) => {
                tracing::warn!(chat = %self.chat_id, subagent = %update.id, %error, "could not record a subagent's update");
            }
        }
    }

    fn settled(&self, rows: DbResult<Vec<SubagentRow>>, failure: &'static str) {
        match rows {
            Ok(rows) => rows.into_iter().for_each(|row| self.publish(row)),
            Err(error) => tracing::warn!(chat = %self.chat_id, %error, "{failure}"),
        }
    }

    fn publish(&self, row: SubagentRow) {
        let _ = self.announce.send(ChatSubagent::from(row));
    }
}

/// The updates that arrived together, folded into one per subagent in the
/// order each subagent was first heard of.
#[derive(Default)]
struct Batch {
    pending: Vec<Pending>,
    positions: HashMap<String, usize>,
}

struct Pending {
    update: Subagent,
    stepped: bool,
}

impl Batch {
    fn add(&mut self, mut update: Subagent, steps: &mut HashMap<String, Vec<SubagentStep>>) {
        let stepped = match update.step.take() {
            Some(step) => {
                upsert(steps.entry(update.id.clone()).or_default(), step);
                true
            }
            None => false,
        };
        match self.positions.get(&update.id) {
            Some(&position) => {
                let pending = &mut self.pending[position];
                pending.stepped |= stepped;
                fold(&mut pending.update, update);
            }
            None => {
                self.positions.insert(update.id.clone(), self.pending.len());
                self.pending.push(Pending { update, stepped });
            }
        }
    }
}

fn upsert(steps: &mut Vec<SubagentStep>, step: SubagentStep) {
    match steps.iter_mut().find(|known| known.id == step.id) {
        Some(known) => *known = step,
        None => steps.push(step),
    }
}

fn fold(earlier: &mut Subagent, later: Subagent) {
    fn newer<T>(earlier: &mut Option<T>, later: Option<T>) {
        if later.is_some() {
            *earlier = later;
        }
    }
    newer(&mut earlier.description, later.description);
    newer(&mut earlier.kind, later.kind);
    newer(&mut earlier.model, later.model);
    newer(&mut earlier.background, later.background);
    newer(&mut earlier.status, later.status);
    newer(&mut earlier.tool_uses, later.tool_uses);
    newer(&mut earlier.duration_ms, later.duration_ms);
    newer(&mut earlier.tokens, later.tokens);
    newer(&mut earlier.last_tool, later.last_tool);
    newer(&mut earlier.summary, later.summary);
    newer(&mut earlier.error, later.error);
}

fn stored(count: u64) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sqlx::PgPool;
    use zone_core::llm::{StepStatus, SubagentStatus};

    use super::*;
    use crate::db::chats::create_chat;

    const PATIENCE: Duration = Duration::from_secs(10);

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

        fn track(&self) -> (Tracker, UnboundedReceiver<ChatSubagent>) {
            let (announce, announced) = mpsc::unbounded_channel();
            let tracker =
                Tracker::spawn(self.state.clone(), self.chat_id, self.message_id, announce);
            (tracker, announced)
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
    }

    async fn chat(pool: &PgPool) -> Uuid {
        create_chat(pool, None, "Subagents", "sonnet", false, true)
            .await
            .expect("a chat")
            .id
    }

    async fn next(announced: &mut UnboundedReceiver<ChatSubagent>) -> ChatSubagent {
        tokio::time::timeout(PATIENCE, announced.recv())
            .await
            .expect("an announcement in time")
            .expect("an announcement")
    }

    /// Every announcement still to come, once the tracker has finished.
    async fn rest(mut announced: UnboundedReceiver<ChatSubagent>) -> Vec<ChatSubagent> {
        let mut rest = Vec::new();
        loop {
            match tokio::time::timeout(PATIENCE, announced.recv()).await {
                Ok(Some(subagent)) => rest.push(subagent),
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
    async fn updates_in_turn_merge_into_one_subagent_whose_steps_are_kept_by_id() {
        let turn = Turn::new().await;
        let (tracker, mut announced) = turn.track();

        tracker.track(&Subagent {
            description: Some("Ask for a review".to_string()),
            kind: Some("general-purpose".to_string()),
            background: Some(true),
            step: Some(step("toolu_01Step", StepStatus::Running)),
            ..Subagent::new("toolu_01Agent")
        });
        next(&mut announced).await;
        tracker.track(&stepped(
            "toolu_01Agent",
            step("toolu_01Step", StepStatus::Succeeded),
        ));
        next(&mut announced).await;
        tracker.track(&Subagent {
            tool_uses: Some(2),
            last_tool: Some("Read".to_string()),
            step: Some(step("toolu_02Step", StepStatus::Running)),
            ..Subagent::new("toolu_01Agent")
        });
        next(&mut announced).await;
        tracker.track(&Subagent {
            summary: Some("Looks good".to_string()),
            ..finished("toolu_01Agent")
        });
        let last = next(&mut announced).await;
        drop(tracker);

        assert_eq!(last.message_id, Some(turn.message_id));
        assert_eq!(last.description, "Ask for a review");
        assert_eq!(last.kind.as_deref(), Some("general-purpose"));
        assert!(last.background);
        assert_eq!(last.tool_uses, Some(2));
        assert_eq!(last.last_tool.as_deref(), Some("Read"));
        assert_eq!(last.summary.as_deref(), Some("Looks good"));
        assert_eq!(last.status, SubagentStatus::Completed);
        assert_eq!(
            last.steps,
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
        let (tracker, announced) = turn.track();
        let steps: Vec<SubagentStep> = (0..20)
            .map(|index| step(&format!("toolu_{index:02}"), StepStatus::Succeeded))
            .collect();

        for step in &steps {
            tracker.track(&stepped("toolu_01Agent", step.clone()));
        }
        tracker.track(&finished("toolu_01Agent"));
        drop(tracker);

        let announcements = rest(announced).await;
        assert_eq!(announcements.len(), 1, "{announcements:?}");
        assert_eq!(announcements[0].steps, steps);
        assert_eq!(announcements[0].status, SubagentStatus::Completed);
    }

    #[tokio::test]
    async fn subagents_reported_together_are_announced_once_each() {
        let turn = Turn::new().await;
        let (tracker, announced) = turn.track();

        tracker.track(&stepped("thread-1", step("call-1", StepStatus::Running)));
        tracker.track(&stepped("thread-2", step("call-2", StepStatus::Running)));
        tracker.track(&stepped("thread-1", step("call-1", StepStatus::Succeeded)));
        tracker.track(&finished("thread-2"));
        tracker.track(&finished("thread-1"));
        drop(tracker);

        let announcements = rest(announced).await;
        let named: Vec<&str> = announcements
            .iter()
            .map(|subagent| subagent.external_id.as_str())
            .collect();
        assert_eq!(
            named,
            ["thread-1", "thread-2"],
            "in the order first heard of"
        );
        assert_eq!(
            announcements[0].steps,
            [step("call-1", StepStatus::Succeeded)]
        );
        assert_eq!(
            announcements[1].steps,
            [step("call-2", StepStatus::Running)]
        );
    }

    #[tokio::test]
    async fn a_new_turn_stops_what_earlier_turns_of_its_chat_left_running() {
        let turn = Turn::new().await;
        let elsewhere = chat(turn.state.db()).await;
        turn.running(turn.chat_id, "earlier", Some(Uuid::new_v4()))
            .await;
        turn.running(turn.chat_id, "unowned", None).await;
        turn.running(elsewhere, "foreign", Some(Uuid::new_v4()))
            .await;

        let (tracker, announced) = turn.track();
        drop(tracker);

        let announcements = rest(announced).await;
        let stopped: Vec<(&str, SubagentStatus)> = announcements
            .iter()
            .map(|subagent| (subagent.external_id.as_str(), subagent.status))
            .collect();
        assert_eq!(
            stopped,
            [
                ("earlier", SubagentStatus::Stopped),
                ("unowned", SubagentStatus::Stopped),
            ]
        );
        assert_eq!(
            turn.status(elsewhere, "foreign").await,
            SubagentStatus::Running.as_str(),
            "another chat's subagents are its own turns' to stop"
        );
    }

    #[tokio::test]
    async fn a_turn_that_ends_stops_the_subagents_it_left_running() {
        let turn = Turn::new().await;
        let (tracker, mut announced) = turn.track();

        tracker.track(&stepped(
            "toolu_01Agent",
            step("toolu_x", StepStatus::Running),
        ));
        let started = next(&mut announced).await;
        drop(tracker);

        assert_eq!(started.status, SubagentStatus::Running);
        let announcements = rest(announced).await;
        assert_eq!(announcements.len(), 1, "{announcements:?}");
        assert_eq!(announcements[0].id, started.id);
        assert_eq!(announcements[0].status, SubagentStatus::Stopped);
        assert!(announcements[0].finished_at.is_some());
    }

    #[tokio::test]
    async fn a_turn_that_heard_of_no_subagent_sweeps_nothing_of_its_own() {
        let turn = Turn::new().await;
        turn.running(turn.chat_id, "toolu_01Agent", Some(turn.message_id))
            .await;

        let (tracker, announced) = turn.track();
        drop(tracker);

        assert_eq!(rest(announced).await, []);
        assert_eq!(
            turn.status(turn.chat_id, "toolu_01Agent").await,
            SubagentStatus::Running.as_str()
        );
    }
}
