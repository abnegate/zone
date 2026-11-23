use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::cluster::Bus;

use super::serving::Serving;
use super::shown::Shown;

/// How often an answering task rewrites a serving key that has not changed.
const REFRESH: Duration = Duration::from_secs(2);

/// How long a serving key outlives its last write, so the key of a replica
/// that died lapses on its own.
const LIFETIME: Duration = Duration::from_secs(6);

/// One answering task's serving key for its chat: what it last wrote and when.
pub struct Beacon {
    bus: Bus,
    chat: Uuid,
    answerer: Uuid,
    shown: Option<Shown>,
}

impl Beacon {
    pub fn new(bus: Bus, chat: Uuid, answerer: Uuid) -> Self {
        Self {
            bus,
            chat,
            answerer,
            shown: None,
        }
    }

    /// Says this replica answers the chat, holding it or not. Writes only a
    /// change, or a refresh once one is due.
    pub async fn show(&mut self, holding: bool) {
        let serving = Serving {
            instance: self.bus.instance(),
            answerer: self.answerer,
            holding,
        };
        if self
            .shown
            .as_ref()
            .is_some_and(|shown| shown.serving == serving && shown.at.elapsed() < REFRESH)
        {
            return;
        }
        if let Some(payload) = serving.write(&self.bus, self.chat, LIFETIME).await {
            self.shown = Some(Shown {
                serving,
                payload,
                at: Instant::now(),
            });
        }
    }

    /// Takes down what this task wrote, leaving a key another task wrote since.
    pub async fn hide(&mut self) {
        if let Some(shown) = self.shown.take() {
            Serving::erase(&self.bus, self.chat, &shown.payload).await;
        }
    }
}
