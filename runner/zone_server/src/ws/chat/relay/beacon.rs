use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::cluster::Bus;

use super::claim::Claim;
use super::serving::Serving;
use super::shown::Shown;

/// How often an answering task rewrites a serving key that has not changed.
const REFRESH: Duration = Duration::from_secs(2);

/// How long a serving key outlives its last write, so the key of a replica
/// that died lapses on its own.
const LIFETIME: Duration = Duration::from_secs(6);

/// One answering task's claim on its chat's serving key: what it last wrote
/// and when.
pub struct Beacon {
    bus: Bus,
    chat: Uuid,
    answerer: Uuid,
    shown: Option<Shown>,
}

impl Beacon {
    pub fn new(bus: Bus, chat: Uuid) -> Self {
        Self {
            bus,
            chat,
            answerer: Uuid::new_v4(),
            shown: None,
        }
    }

    /// Keeps this task's claim on the chat, holding it or not: rewrites the
    /// key on a change, or once a refresh is due. Says whether this task
    /// answers the chat.
    pub async fn show(&mut self, holding: bool) -> bool {
        if self
            .shown
            .as_ref()
            .is_some_and(|shown| shown.serving.holding == holding && shown.at.elapsed() < REFRESH)
        {
            return true;
        }
        self.claim(holding).await
    }

    /// Claims the chat for this task now, and says whether it answers the
    /// chat: the key names it, or Valkey cannot say, so the replica goes by
    /// what it holds itself.
    pub async fn claim(&mut self, holding: bool) -> bool {
        let serving = Serving {
            instance: self.bus.instance(),
            answerer: self.answerer,
            holding,
        };
        match serving.claim(&self.bus, self.chat, LIFETIME).await {
            Claim::Taken(payload) => {
                self.shown = Some(Shown {
                    serving,
                    payload,
                    at: Instant::now(),
                });
                true
            }
            Claim::Held => {
                self.shown = None;
                false
            }
            Claim::Unknown => true,
        }
    }

    /// Takes down what this task wrote, leaving a key another task wrote since.
    pub async fn hide(&mut self) {
        if let Some(shown) = self.shown.take() {
            Serving::erase(&self.bus, self.chat, &shown.payload).await;
        }
    }
}
