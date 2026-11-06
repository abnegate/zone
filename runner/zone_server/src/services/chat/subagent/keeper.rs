use std::sync::Arc;

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use uuid::Uuid;
use zone_core::llm::{Subagent, SubagentStep};

use super::batch::Batch;
use super::pending::Pending;
use super::{Announcer, ChatSubagent};
use crate::db::DbResult;
use crate::db::chat_subagents::{self, Stopped, SubagentChange, SubagentRow};
use crate::state::AppState;

/// Writes one turn's subagent updates as they arrive and announces each write.
#[derive(Clone)]
pub(super) struct Keeper {
    state: AppState,
    chat_id: Uuid,
    message_id: Uuid,
    announcer: Arc<dyn Announcer>,
    /// Held closed until a test has queued every update it means to send.
    #[cfg(test)]
    pub(super) hold: Option<Arc<tokio::sync::Notify>>,
}

impl Keeper {
    pub(super) fn new(
        state: AppState,
        chat_id: Uuid,
        message_id: Uuid,
        announcer: Arc<dyn Announcer>,
    ) -> Self {
        Self {
            state,
            chat_id,
            message_id,
            announcer,
            #[cfg(test)]
            hold: None,
        }
    }

    /// Starts keeping the updates sent to the returned sender, until every
    /// clone of it is gone.
    pub(super) fn start(self) -> UnboundedSender<Subagent> {
        let (updates, received) = mpsc::unbounded_channel();
        tokio::spawn(self.run(received));
        updates
    }

    async fn run(self, mut updates: UnboundedReceiver<Subagent>) {
        let others =
            chat_subagents::stop_others(self.state.db(), self.chat_id, self.message_id).await;
        self.settled(
            others,
            "could not stop the subagents an earlier turn left running",
        )
        .await;

        #[cfg(test)]
        if let Some(hold) = &self.hold {
            hold.notified().await;
        }

        while let Some(first) = updates.recv().await {
            let mut batch = Batch::default();
            batch.add(first);
            while let Ok(next) = updates.try_recv() {
                batch.add(next);
            }
            for pending in batch.into_pending() {
                self.record(pending).await;
            }
        }

        let stopped = chat_subagents::stop(self.state.db(), self.chat_id, self.message_id).await;
        self.settled(stopped, "could not stop the subagents a turn left running")
            .await;
    }

    async fn record(&self, pending: Pending) {
        match self.write(&pending).await {
            Ok((row, steps)) => {
                self.announcer
                    .announce(ChatSubagent::from(row), steps)
                    .await;
            }
            Err(error) => {
                tracing::warn!(chat = %self.chat_id, subagent = %pending.update.id, %error, "could not record a subagent's update");
            }
        }
    }

    /// One upsert of the subagent and, when the burst touched any of its
    /// steps, one of those steps in the same transaction.
    async fn write(&self, pending: &Pending) -> DbResult<(SubagentRow, Vec<SubagentStep>)> {
        let update = &pending.update;
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
        };
        if pending.steps.is_empty() {
            let row = chat_subagents::record(self.state.db(), &change).await?;
            return Ok((row, Vec::new()));
        }
        let mut transaction = self.state.db().begin().await?;
        let mut row = chat_subagents::record(&mut *transaction, &change).await?;
        let steps = chat_subagents::record_steps(&mut *transaction, row.id, &pending.steps).await?;
        transaction.commit().await?;
        row.step_count += steps.added;
        Ok((row, steps.changed))
    }

    async fn settled(&self, stopped: DbResult<Vec<Stopped>>, failure: &'static str) {
        match stopped {
            Ok(stopped) => {
                for Stopped { subagent, failed } in stopped {
                    self.announcer
                        .announce(ChatSubagent::from(subagent), failed.0)
                        .await;
                }
            }
            Err(error) => tracing::warn!(chat = %self.chat_id, %error, "{failure}"),
        }
    }
}

fn stored(count: u64) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}
