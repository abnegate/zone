use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use abnegate_agent_cli::{Subagent, SubagentStatus, SubagentStep};
use sqlx::{Acquire, PgConnection, Postgres, Transaction};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use uuid::Uuid;

use super::batch::Batch;
use super::pending::Pending;
use super::{Announcer, ChatSubagent};
use crate::db::DbResult;
use crate::db::chat_subagents::{self, Stopped, SubagentChange, SubagentRow};
use crate::state::AppState;

/// The SQLSTATE classes of a failure the same statement can get past on a
/// later try: connection exceptions, rolled-back transactions, insufficient
/// resources and operator intervention.
const TRANSIENT_CLASSES: [&str; 4] = ["08", "40", "53", "57"];

/// How many times a turn's final transaction is tried before what it left
/// running is left to the sweep. Another stop of the same rows can abort it
/// as a deadlock, and an outage can outlast a single try.
pub(super) const FINISH_ATTEMPTS: u32 = 3;

/// How much longer each try of a turn's final transaction waits than the last.
const FINISH_BACKOFF: Duration = Duration::from_millis(100);

/// A subagent as a write left it, with the steps that write changed.
type Written = (SubagentRow, Vec<SubagentStep>);

/// Writes one turn's subagent updates as they arrive and announces each write.
pub(super) struct Keeper {
    state: AppState,
    chat_id: Uuid,
    message_id: Uuid,
    announcer: Arc<dyn Announcer>,
    /// The subagents last announced running, which readers still show running
    /// until they hear otherwise.
    running: HashSet<Uuid>,
    /// The updates a write failed to store, kept in order for the next one.
    retained: Vec<Pending>,
    /// Held closed until a test has queued every update it means to send.
    #[cfg(test)]
    pub(super) hold: Option<Arc<tokio::sync::Notify>>,
    /// How many transactions a test has made fail as an outage would.
    #[cfg(test)]
    pub(super) outage: Arc<std::sync::atomic::AtomicUsize>,
    /// How many final transactions a test has made deadlock just before
    /// their stop.
    #[cfg(test)]
    pub(super) deadlocks: Arc<std::sync::atomic::AtomicUsize>,
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
            running: HashSet::new(),
            retained: Vec::new(),
            #[cfg(test)]
            hold: None,
            #[cfg(test)]
            outage: Arc::default(),
            #[cfg(test)]
            deadlocks: Arc::default(),
        }
    }

    /// Starts keeping the updates sent to the returned sender, until every
    /// clone of it is gone.
    pub(super) fn start(self) -> UnboundedSender<Subagent> {
        let (updates, received) = mpsc::unbounded_channel();
        tokio::spawn(self.run(received));
        updates
    }

    async fn run(mut self, mut updates: UnboundedReceiver<Subagent>) {
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
            let mut batch = Batch::from(std::mem::take(&mut self.retained));
            batch.add(first);
            while let Ok(next) = updates.try_recv() {
                batch.add(next);
            }
            self.record_all(batch).await;
        }
        if !self.retained.is_empty() {
            let batch = Batch::from(std::mem::take(&mut self.retained));
            self.record_all(batch).await;
        }
        match self.finish().await {
            Ok(written) => {
                for (row, steps) in written {
                    self.announce(ChatSubagent::from(row), steps).await;
                }
            }
            Err(error) => {
                tracing::warn!(chat = %self.chat_id, lost = self.retained.len(), %error, "could not record a turn's last subagent updates or stop the subagents it left running");
            }
        }
        self.reconcile().await;
    }

    async fn record_all(&mut self, batch: Batch) {
        for pending in batch.into_pending() {
            self.record(pending).await;
        }
    }

    async fn record(&mut self, pending: Pending) {
        match self.write(&pending).await {
            Ok(Some((row, steps))) => self.announce(ChatSubagent::from(row), steps).await,
            Ok(None) => {}
            Err(error) if transient(&error) => {
                tracing::warn!(chat = %self.chat_id, subagent = %pending.update.id, %error, "could not record a subagent's update yet");
                self.retained.push(pending);
            }
            Err(error) => {
                tracing::warn!(chat = %self.chat_id, subagent = %pending.update.id, %error, "could not record a subagent's update");
            }
        }
    }

    /// One upsert of the subagent and, when the burst touched any of its
    /// steps, one of those steps, in one transaction that holds the turn so it
    /// is not deleted underneath. Nothing is written for a turn already gone.
    async fn write(&self, pending: &Pending) -> DbResult<Option<Written>> {
        #[cfg(test)]
        self.outage()?;
        let mut transaction = self.state.db().begin().await?;
        if !chat_subagents::hold_turn(&mut *transaction, self.chat_id, self.message_id).await? {
            return Ok(None);
        }
        let written = self.store(&mut transaction, pending).await?;
        transaction.commit().await?;
        Ok(Some(written))
    }

    /// Writes the updates still retained and stops the subagents the turn left
    /// running, all in one transaction, and returns every subagent it changed
    /// in the order it changed them. A reported outcome is written before the
    /// stop, so the stop never hides it. A transaction that fails on what a
    /// later try can get past is tried again, up to [`FINISH_ATTEMPTS`] in
    /// all; when the last fails, neither lands, and the turn's subagents stay
    /// running for the sweep to stop.
    async fn finish(&self) -> DbResult<Vec<Written>> {
        let mut attempt = 1;
        loop {
            match self.finish_once().await {
                Err(error) if attempt < FINISH_ATTEMPTS && transient(&error) => {
                    tracing::warn!(chat = %self.chat_id, attempt, %error, "could not finish a turn's subagents yet");
                    tokio::time::sleep(FINISH_BACKOFF * attempt).await;
                    attempt += 1;
                }
                finished => return finished,
            }
        }
    }

    async fn finish_once(&self) -> DbResult<Vec<Written>> {
        #[cfg(test)]
        self.outage()?;
        let mut transaction = self.state.db().begin().await?;
        let mut written = Vec::with_capacity(self.retained.len());
        if chat_subagents::hold_turn(&mut *transaction, self.chat_id, self.message_id).await? {
            for pending in &self.retained {
                written.extend(self.store_alone(&mut transaction, pending).await?);
            }
        }
        #[cfg(test)]
        self.deadlock(&mut transaction).await?;
        let stopped =
            chat_subagents::stop(&mut *transaction, self.chat_id, self.message_id).await?;
        transaction.commit().await?;
        written.extend(
            stopped
                .into_iter()
                .map(|Stopped { subagent, failed }| (subagent, failed.0)),
        );
        Ok(written)
    }

    /// Stores `pending` in a savepoint of `transaction`, so an update the
    /// database refuses for good is dropped alone, and returns what it wrote,
    /// or `None` when it was dropped.
    async fn store_alone(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        pending: &Pending,
    ) -> DbResult<Option<Written>> {
        let mut savepoint = transaction.begin().await?;
        match self.store(&mut savepoint, pending).await {
            Ok(written) => {
                savepoint.commit().await?;
                Ok(Some(written))
            }
            Err(error) if transient(&error) => Err(error),
            Err(error) => {
                savepoint.rollback().await?;
                tracing::warn!(chat = %self.chat_id, subagent = %pending.update.id, %error, "could not record a subagent's update");
                Ok(None)
            }
        }
    }

    async fn store(&self, connection: &mut PgConnection, pending: &Pending) -> DbResult<Written> {
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
            duration_ms: update.duration.map(milliseconds),
            tokens: update.tokens.map(stored),
            last_tool: update.last_tool.as_deref(),
            summary: update.summary.as_deref(),
            error: update.error.as_deref(),
        };
        let mut row = chat_subagents::record(&mut *connection, &change).await?;
        let recorded =
            chat_subagents::record_steps(&mut *connection, row.id, &pending.steps).await?;
        let mut steps = recorded.changed;
        if update.status.is_some_and(SubagentStatus::finished)
            && row.status != SubagentStatus::Running.as_str()
        {
            steps.extend(chat_subagents::fail_running_steps(&mut *connection, row.id).await?);
        }
        row.step_count += recorded.added;
        Ok((row, steps))
    }

    #[cfg(test)]
    fn outage(&self) -> DbResult<()> {
        match self.outage.try_update(
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
            |left| left.checked_sub(1),
        ) {
            Ok(_) => Err(sqlx::Error::PoolTimedOut),
            Err(_) => Ok(()),
        }
    }

    #[cfg(test)]
    async fn deadlock(&self, connection: &mut PgConnection) -> DbResult<()> {
        let due = self.deadlocks.try_update(
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
            |left| left.checked_sub(1),
        );
        if due.is_ok() {
            sqlx::query(
                "DO $$ BEGIN RAISE EXCEPTION 'deadlock detected' USING ERRCODE = 'deadlock_detected'; END $$",
            )
            .execute(connection)
            .await?;
        }
        Ok(())
    }

    async fn settled(&mut self, stopped: DbResult<Vec<Stopped>>, failure: &'static str) {
        match stopped {
            Ok(stopped) => {
                for Stopped { subagent, failed } in stopped {
                    self.announce(ChatSubagent::from(subagent), failed.0).await;
                }
            }
            Err(error) => tracing::warn!(chat = %self.chat_id, %error, "{failure}"),
        }
    }

    /// Announces what became of the subagents readers were last told are
    /// running but that something other than this keeper settled, such as the
    /// turn's own interruption, so none is left showing as running.
    async fn reconcile(&mut self) {
        if self.running.is_empty() {
            return;
        }
        let ids: Vec<Uuid> = self.running.iter().copied().collect();
        let settled = chat_subagents::settled(self.state.db(), self.chat_id, &ids).await;
        self.settled(
            settled,
            "could not read the subagents a turn's settling stopped",
        )
        .await;
    }

    async fn announce(&mut self, subagent: ChatSubagent, steps: Vec<SubagentStep>) {
        if subagent.status == SubagentStatus::Running {
            self.running.insert(subagent.id);
        } else {
            self.running.remove(&subagent.id);
        }
        self.announcer.announce(subagent, steps).await;
    }
}

fn transient(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(error) => error.code().is_some_and(|code| {
            TRANSIENT_CLASSES
                .iter()
                .any(|class| code.starts_with(class))
        }),
        sqlx::Error::Io(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => true,
        _ => false,
    }
}

fn stored(count: u64) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// `duration` as the milliseconds a subagent's run is stored in.
fn milliseconds(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}
