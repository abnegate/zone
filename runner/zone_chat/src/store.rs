//! The storage a chat session needs, and nothing about how it is stored.
//!
//! A session leases the right to append to one conversation, writes turns
//! through that lease, and reads history back. Postgres is one implementation;
//! the session code never names it.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::history::{Evidence, History, NewEntry, ReplayMessage, Summary};

#[derive(Debug, Error)]
pub enum Error {
    #[error("This chat already has an active response")]
    Busy,
    #[error("Chat generation ownership expired or changed; the response was stopped")]
    LeaseLost,
    #[error("Conversation checkpoint changed; retry from current history")]
    Conflict,
    #[error("Conversation integrity error: {0}")]
    Integrity(String),
    #[error("Conversation evidence was not found in this chat")]
    NotFound,
    #[error("conversation store failed: {0}")]
    Backend(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct Lease {
    pub chat_id: Uuid,
    pub owner: Uuid,
    pub fence: i64,
    pub expires_at: DateTime<Utc>,
}

/// An independently scheduled renewal, unaffected by blocked websocket sends or tools.
/// Dropping the guard stops renewal; callers release explicitly after durable completion.
pub struct Guard {
    lease: Lease,
    lost: watch::Receiver<bool>,
    task: Option<JoinHandle<()>>,
}

impl Guard {
    pub async fn stop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }
    pub fn lease(&self) -> &Lease {
        &self.lease
    }
    pub fn is_lost(&self) -> bool {
        *self.lost.borrow()
    }
    pub async fn lost(&mut self) {
        if self.is_lost() {
            return;
        }
        let _ = self.lost.wait_for(|lost| *lost).await;
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// A message as the conversation store holds it.
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub title_claimed: bool,
    pub id: Uuid,
    pub chat_id: Uuid,
    pub role: String,
    pub content: String,
    pub metadata: Option<Value>,
    pub created_at: Option<chrono::NaiveDateTime>,
}

/// The durable conversation behind one chat.
///
/// Writes are gated on a [`Lease`], so a chat can only ever have one live
/// response: whoever holds the lease owns the turn, and a stale holder is told
/// [`Error::LeaseLost`] rather than being allowed to append.
#[async_trait]
pub trait ContextStore: Send + Sync {
    /// Take the right to respond in this chat, or fail with [`Error::Busy`].
    async fn acquire(&self, owner: Uuid, lifetime: Duration) -> Result<Lease, Error>;

    /// Extend a lease that is still ours.
    async fn renew(&self, lease: &Lease, lifetime: Duration) -> Result<Lease, Error>;

    /// Fail unless this lease is still the current one.
    async fn assert_current(&self, lease: &Lease) -> Result<(), Error>;

    /// Give the lease up. False when it had already been taken over.
    async fn release(&self, lease: &Lease) -> Result<bool, Error>;

    /// Record the user message that opens a turn.
    async fn begin(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        user_message_id: Uuid,
        content: &str,
        metadata: Option<Value>,
        message: ReplayMessage,
    ) -> Result<StoredMessage, Error>;

    /// Append entries produced while the turn runs.
    async fn append(&self, lease: &Lease, turn_id: Uuid, entries: &[NewEntry])
    -> Result<(), Error>;

    async fn create_message(
        &self,
        lease: &Lease,
        role: &str,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<StoredMessage, Error>;

    async fn delete_message(&self, lease: &Lease, id: Uuid) -> Result<bool, Error>;

    /// Mark evidence as folded into the visible history.
    async fn consumed(&self, lease: &Lease, ids: &[String]) -> Result<(), Error>;

    async fn complete(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<StoredMessage, Error>;

    async fn publish(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<StoredMessage, Error>;

    /// Close a turn, durably, whether it ran to the end or was interrupted.
    async fn finish(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        content: &str,
        metadata: Option<Value>,
        interrupted: bool,
        partial: Option<&ReplayMessage>,
    ) -> Result<StoredMessage, Error>;

    async fn interrupt(&self, lease: &Lease, turn_id: Uuid) -> Result<(), Error>;

    /// Settle turns a previous process left open. Returns how many.
    async fn recover(&self, lease: &Lease) -> Result<usize, Error>;

    async fn load(&self) -> Result<History, Error>;

    /// Replace the summary, refusing when someone else moved it first.
    async fn checkpoint(
        &self,
        lease: &Lease,
        expected: Option<&Summary>,
        proposed: &Summary,
    ) -> Result<(), Error>;

    async fn evidence(&self, id: &str, offset: u64, limit: u64) -> Result<Evidence, Error>;

    async fn catalog(&self, offset: u64, limit: u64) -> Result<Evidence, Error>;
}

/// Renew a lease on its own schedule, so a blocked websocket send or a slow
/// tool cannot let it lapse mid-turn. Dropping the guard stops renewal;
/// callers still release explicitly once the turn is durable.
pub fn keep_alive(
    store: Arc<dyn ContextStore>,
    lease: Lease,
    lifetime: Duration,
) -> Result<Guard, Error> {
    if lifetime.is_zero() {
        return Err(Error::Integrity("lease lifetime must be non-zero".into()));
    }
    let (tx, lost) = watch::channel(false);
    let mut current = lease.clone();
    let task = tokio::spawn(async move {
        let interval = lifetime / 3;
        loop {
            tokio::time::sleep(interval).await;
            match store.renew(&current, lifetime).await {
                Ok(next) => current = next,
                Err(_) => {
                    let _ = tx.send(true);
                    return;
                }
            }
        }
    });
    Ok(Guard {
        lease,
        lost,
        task: Some(task),
    })
}
