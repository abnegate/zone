//! Progress and terminal frames for a task run.
//!
//! A run's progress is published by whoever advances or finishes it -- a
//! worker, a route, the lease sweeper -- and read by whoever is watching: a
//! websocket, or a `wait_for` parked on the run. The hub therefore sits below
//! all of them rather than inside any one reader, so the writers do not have
//! to reach up into a transport to announce what they did.

use once_cell::sync::Lazy;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::db::tasks;

/// What a failed run is reported as when it recorded no reason of its own.
pub const UNKNOWN_ERROR: &str = "Unknown error";

/// Progress message sent to clients
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressMessage {
    /// Initial task run state
    Init {
        run_id: Uuid,
        task_id: Uuid,
        status: String,
    },
    /// Status changed
    StatusUpdate {
        status: String,
        current_phase: Option<String>,
        progress_percent: Option<i32>,
    },
    /// New log entry
    Log {
        id: Uuid,
        phase: String,
        agent_type: String,
        log_level: String,
        message: String,
        /// Receipt detail the worker attached to this line.
        metadata: Option<serde_json::Value>,
    },
    /// Task completed successfully
    Completed { status: String },
    /// Task failed
    Failed { error: String },
    /// A person stopped the run before it finished
    Cancelled { message: String },
    /// Error message
    Error { message: String },
}

impl ProgressMessage {
    /// The frame that ends a run with this status, or `None` while it is still
    /// running or waiting.
    pub fn terminal(status: &str, error: Option<&str>) -> Option<Self> {
        match status {
            tasks::COMPLETED => Some(Self::Completed {
                status: status.to_string(),
            }),
            tasks::FAILED => Some(Self::Failed {
                error: error.unwrap_or(UNKNOWN_ERROR).to_string(),
            }),
            tasks::CANCELLED => Some(Self::Cancelled {
                message: error.unwrap_or(tasks::STOPPED).to_string(),
            }),
            _ => None,
        }
    }
}

/// Global task progress broadcaster
///
/// In production, this would be backed by Redis pub/sub for horizontal scaling
pub struct TaskProgressBroadcaster {
    senders: dashmap::DashMap<Uuid, broadcast::Sender<ProgressMessage>>,
}

impl TaskProgressBroadcaster {
    pub fn new() -> Self {
        Self {
            senders: dashmap::DashMap::new(),
        }
    }

    /// Get or create a broadcast channel for a task run
    pub fn get_sender(&self, run_id: Uuid) -> broadcast::Sender<ProgressMessage> {
        self.senders
            .entry(run_id)
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(100);
                tx
            })
            .clone()
    }

    /// Subscribe to a task run's progress
    pub fn subscribe(&self, run_id: Uuid) -> broadcast::Receiver<ProgressMessage> {
        self.get_sender(run_id).subscribe()
    }

    /// Broadcast a message to all subscribers of a task run
    pub fn broadcast(&self, run_id: Uuid, message: ProgressMessage) {
        if let Some(sender) = self.senders.get(&run_id) {
            let _ = sender.send(message);
        }
    }

    /// Remove a broadcast channel when no longer needed
    pub fn remove(&self, run_id: Uuid) {
        self.senders.remove(&run_id);
    }

    /// Whether a run still holds a channel here.
    ///
    /// A terminal frame drops it, so this is how a caller distinguishes a run
    /// nobody has finished from one whose channel has already been released.
    /// Reading it does not create one, which [`Self::get_sender`] would.
    pub fn tracks(&self, run_id: Uuid) -> bool {
        self.senders.contains_key(&run_id)
    }
}

impl Default for TaskProgressBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

/// The process's one broadcaster, so the writers that finish a run can publish
/// without a handle threaded through every worker and route that finishes one.
/// `AppState` holds this same instance rather than a second of its own.
static PROGRESS: Lazy<Arc<TaskProgressBroadcaster>> =
    Lazy::new(|| Arc::new(TaskProgressBroadcaster::new()));

pub fn progress() -> Arc<TaskProgressBroadcaster> {
    PROGRESS.clone()
}

/// Announce that a run reached a terminal status, then drop its channel.
///
/// Called from every writer that can end a run, not only the owner completing
/// it, because a waiter subscribed to a run the sweeper reaps or a person stops
/// would otherwise hang until its own deadline. Removing the sender right after
/// the send keeps the map from growing one entry per run for the life of the
/// process; a receiver already holding the frame still reads it.
pub fn publish_terminal(run: Uuid, status: &str, error: Option<&str>) {
    let broadcaster = progress();
    if let Some(message) = ProgressMessage::terminal(status, error) {
        broadcaster.broadcast(run, message);
    }
    broadcaster.remove(run);
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNING: &str = "running";
    const WAITING: &str = "waiting";
    const FAILURE: &str = "the build broke";

    #[test]
    fn a_cancelled_run_ends_with_a_cancelled_frame_carrying_its_reason() {
        match ProgressMessage::terminal(tasks::CANCELLED, Some(tasks::STOPPED)) {
            Some(ProgressMessage::Cancelled { message }) => assert_eq!(message, tasks::STOPPED),
            other => panic!("expected a cancelled frame for a cancelled run, got {other:?}"),
        }
    }

    #[test]
    fn a_cancelled_run_without_a_reason_reads_as_stopped() {
        match ProgressMessage::terminal(tasks::CANCELLED, None) {
            Some(ProgressMessage::Cancelled { message }) => assert_eq!(message, tasks::STOPPED),
            other => panic!("expected a cancelled frame for a cancelled run, got {other:?}"),
        }
    }

    #[test]
    fn a_cancelled_frame_serializes_with_its_own_type() {
        let frame = ProgressMessage::terminal(tasks::CANCELLED, Some(tasks::STOPPED))
            .expect("a cancelled run ends the stream");
        assert_eq!(
            serde_json::to_value(frame).unwrap(),
            serde_json::json!({"type": "cancelled", "message": tasks::STOPPED})
        );
    }

    #[test]
    fn a_failed_run_ends_with_its_error() {
        match ProgressMessage::terminal(tasks::FAILED, Some(FAILURE)) {
            Some(ProgressMessage::Failed { error }) => assert_eq!(error, FAILURE),
            other => panic!("expected a failed frame for a failed run, got {other:?}"),
        }
    }

    #[test]
    fn a_failed_run_without_an_error_reports_an_unknown_error() {
        match ProgressMessage::terminal(tasks::FAILED, None) {
            Some(ProgressMessage::Failed { error }) => assert_eq!(error, UNKNOWN_ERROR),
            other => panic!("expected a failed frame for a failed run, got {other:?}"),
        }
    }

    #[test]
    fn a_completed_run_ends_as_completed() {
        match ProgressMessage::terminal(tasks::COMPLETED, None) {
            Some(ProgressMessage::Completed { status }) => assert_eq!(status, tasks::COMPLETED),
            other => panic!("expected a completed frame, got {other:?}"),
        }
    }

    #[test]
    fn an_active_run_has_no_terminal_frame() {
        for status in [RUNNING, WAITING] {
            assert!(
                ProgressMessage::terminal(status, None).is_none(),
                "a {status} run must not end the stream"
            );
        }
    }

    #[tokio::test]
    async fn a_stop_reaches_a_subscriber_as_cancelled_and_releases_the_channel() {
        let run = Uuid::new_v4();
        let mut events = progress().subscribe(run);
        publish_terminal(run, tasks::CANCELLED, Some(tasks::STOPPED));
        match events.recv().await {
            Ok(ProgressMessage::Cancelled { message }) => assert_eq!(message, tasks::STOPPED),
            other => panic!("expected a cancelled frame, got {other:?}"),
        }
        assert!(
            !progress().tracks(run),
            "a terminal frame drops the channel"
        );
    }
}
