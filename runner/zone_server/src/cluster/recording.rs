use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::watch;

use super::command::Command;

/// A transport that reaches nobody and writes down every change to the
/// channels it listens on, in the order the bus asked for them, as the
/// pub/sub connection would send them to Valkey. `unsubscribing` runs as each
/// unsubscribe is asked for, before it is written down.
pub struct Recording {
    commands: Mutex<Vec<Command>>,
    unsubscribing: Box<dyn Fn() + Send + Sync>,
}

impl Recording {
    pub fn new(unsubscribing: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            commands: Mutex::new(Vec::new()),
            unsubscribing: Box::new(unsubscribing),
        }
    }

    pub fn subscribe(&self, channel: Arc<str>, confirmed: watch::Sender<bool>) {
        confirmed.send_replace(true);
        self.record(Command::Subscribe { channel, confirmed });
    }

    pub fn unsubscribe(&self, channel: Arc<str>) {
        (self.unsubscribing)();
        self.record(Command::Unsubscribe { channel });
    }

    /// Each channel change so far, as `subscribe <channel>` or `unsubscribe <channel>`.
    pub fn changes(&self) -> Vec<String> {
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|command| match command {
                Command::Subscribe { channel, .. } => format!("subscribe {channel}"),
                Command::Unsubscribe { channel } => format!("unsubscribe {channel}"),
            })
            .collect()
    }

    fn record(&self, command: Command) {
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(command);
    }
}

impl fmt::Debug for Recording {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Recording")
            .field("changes", &self.changes())
            .finish_non_exhaustive()
    }
}
