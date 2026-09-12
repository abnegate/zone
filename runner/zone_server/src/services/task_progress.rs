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

/// The status a finished run reports as a success; anything else is a failure.
const COMPLETED: &str = "completed";

/// What a failed run is reported as when it recorded no reason of its own.
const UNKNOWN_ERROR: &str = "Unknown error";

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
    /// Error message
    Error { message: String },
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
/// Called from both writers that can end a run -- the owner completing it and
/// the sweeper reaping its lease -- because a waiter subscribed to a run the
/// sweeper reaps would otherwise hang until its own deadline. Removing the
/// sender right after the send keeps the map from growing one entry per run for
/// the life of the process; a receiver already holding the frame still reads it.
pub fn publish_terminal(run: Uuid, status: &str, error: Option<&str>) {
    let message = if status == COMPLETED {
        ProgressMessage::Completed {
            status: status.to_string(),
        }
    } else {
        ProgressMessage::Failed {
            error: error.unwrap_or(UNKNOWN_ERROR).to_string(),
        }
    };
    let broadcaster = progress();
    broadcaster.broadcast(run, message);
    broadcaster.remove(run);
}
