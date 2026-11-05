//! Messages sent while a chat's turn is running.
//!
//! The queue lives beside the turn it waits on, on the server, so a reload or a
//! second window sees the same queue, and it drains whether or not anybody is
//! connected when the turn ends.

use std::collections::VecDeque;
use std::future::Future;

use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::Serialize;
use uuid::Uuid;

static QUEUES: Lazy<DashMap<Uuid, Queue>> = Lazy::new(DashMap::new);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Queued {
    pub id: Uuid,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// The turn runs as whoever sent the message, not whoever is connected when it starts.
    #[serde(skip)]
    pub author: Uuid,
}

#[derive(Debug, PartialEq)]
pub enum Finished {
    Next(Queued),
    Held,
    Idle,
}

#[derive(Debug, PartialEq)]
pub enum Interrupted {
    /// The message is next; the running turn has to be stopped for it to start.
    Cancel,
    Start(Queued),
    Missing,
}

#[derive(Debug, Default)]
pub struct Queue {
    messages: VecDeque<Queued>,
    running: bool,
    stopping: bool,
    held: bool,
}

impl Queue {
    /// Queue a message and start it if nothing is running. `busy` is a turn
    /// this queue did not start, such as an automation's.
    pub fn submit(&mut self, message: Queued, busy: bool) -> Option<Queued> {
        self.messages.push_back(message);
        if self.running || busy {
            return None;
        }
        self.held = false;
        self.start()
    }

    /// The turn this queue started has ended. A turn the user stopped holds
    /// what is left, so Stop does not immediately start the next message.
    pub fn finish(&mut self) -> Finished {
        if std::mem::take(&mut self.stopping) || self.held {
            self.running = false;
            self.held = !self.messages.is_empty();
            return if self.held {
                Finished::Held
            } else {
                Finished::Idle
            };
        }
        self.start().map_or(Finished::Idle, Finished::Next)
    }

    /// A turn this queue did not start has ended. While one it did start is
    /// running, that turn drains the queue when it ends.
    pub fn release(&mut self) -> Finished {
        if self.running {
            return Finished::Idle;
        }
        self.finish()
    }

    pub fn stop(&mut self, busy: bool) {
        if self.running || busy {
            self.stopping = true;
        }
    }

    pub fn interrupt(&mut self, id: Uuid, busy: bool) -> Interrupted {
        let Some(index) = self.messages.iter().position(|message| message.id == id) else {
            return Interrupted::Missing;
        };
        if let Some(message) = self.messages.remove(index) {
            self.messages.push_front(message);
        }
        self.held = false;
        if self.running || busy {
            self.stopping = false;
            return Interrupted::Cancel;
        }
        self.start()
            .map_or(Interrupted::Missing, Interrupted::Start)
    }

    pub fn edit(&mut self, id: Uuid, content: String) -> bool {
        let Some(message) = self.messages.iter_mut().find(|message| message.id == id) else {
            return false;
        };
        message.content = content;
        true
    }

    pub fn remove(&mut self, id: Uuid) -> bool {
        let before = self.messages.len();
        self.messages.retain(|message| message.id != id);
        if self.messages.is_empty() {
            self.held = false;
        }
        self.messages.len() != before
    }

    pub fn messages(&self) -> Vec<Queued> {
        self.messages.iter().cloned().collect()
    }

    pub fn held(&self) -> bool {
        self.held
    }

    fn start(&mut self) -> Option<Queued> {
        let next = self.messages.pop_front();
        self.running = next.is_some();
        self.stopping = false;
        next
    }

    fn is_idle(&self) -> bool {
        !self.running && self.messages.is_empty()
    }
}

/// Change a chat's queue. Never held across an await: the map's shard lock is
/// synchronous.
pub fn with<T>(chat_id: Uuid, change: impl FnOnce(&mut Queue) -> T) -> T {
    let mut queue = QUEUES.entry(chat_id).or_default();
    let result = change(&mut queue);
    drop(queue);
    QUEUES.remove_if(&chat_id, |_, queue| queue.is_idle());
    result
}

pub fn snapshot(chat_id: Uuid) -> Option<(Vec<Queued>, bool)> {
    QUEUES
        .get(&chat_id)
        .filter(|queue| !queue.messages.is_empty())
        .map(|queue| (queue.messages(), queue.held()))
}

/// Run `first`, then every message queued behind it, until the queue is empty
/// or held. `announce` is told which message left the queue to start, or
/// `None` when the queue came to rest holding messages.
pub async fn drain<Turn, Running, Announce, Announced>(
    chat_id: Uuid,
    first: Queued,
    mut turn: Turn,
    mut announce: Announce,
) where
    Turn: FnMut(Queued) -> Running,
    Running: Future<Output = ()>,
    Announce: FnMut(Option<Uuid>) -> Announced,
    Announced: Future<Output = ()>,
{
    let mut running = turn(first);
    loop {
        running.await;
        match with(chat_id, Queue::finish) {
            Finished::Next(next) => {
                let id = next.id;
                running = turn(next);
                announce(Some(id)).await;
            }
            Finished::Held => {
                announce(None).await;
                return;
            }
            Finished::Idle => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn queued(content: &str) -> Queued {
        Queued {
            id: Uuid::new_v4(),
            content: content.into(),
            metadata: None,
            author: Uuid::nil(),
        }
    }

    fn contents(queue: &Queue) -> Vec<String> {
        queue.messages().into_iter().map(|m| m.content).collect()
    }

    #[test]
    fn a_message_starts_at_once_when_nothing_is_running() {
        let mut queue = Queue::default();
        let first = queued("first");
        assert_eq!(queue.submit(first.clone(), false), Some(first));
        assert!(queue.messages().is_empty());
    }

    #[test]
    fn a_message_sent_during_a_turn_waits_and_runs_next_in_order() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        assert_eq!(queue.submit(queued("second"), false), None);
        assert_eq!(queue.submit(queued("third"), false), None);
        assert_eq!(contents(&queue), ["second", "third"]);

        let Finished::Next(next) = queue.finish() else {
            panic!("the queue drains when the turn ends");
        };
        assert_eq!(next.content, "second");
        let Finished::Next(next) = queue.finish() else {
            panic!("the queue drains when the turn ends");
        };
        assert_eq!(next.content, "third");
        assert_eq!(queue.finish(), Finished::Idle);
    }

    #[test]
    fn a_turn_someone_else_started_makes_a_message_wait() {
        let mut queue = Queue::default();
        assert_eq!(queue.submit(queued("typed"), true), None);
        let Finished::Next(next) = queue.release() else {
            panic!("the automation's end starts the queue");
        };
        assert_eq!(next.content, "typed");
        assert_eq!(
            queue.release(),
            Finished::Idle,
            "the queue's own turn drains it"
        );
    }

    #[test]
    fn a_stopped_turn_holds_the_queue_until_the_next_send() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        queue.submit(queued("second"), false);
        queue.stop(false);
        assert_eq!(queue.finish(), Finished::Held);
        assert!(queue.held());
        assert_eq!(contents(&queue), ["second"]);

        let Some(next) = queue.submit(queued("third"), false) else {
            panic!("a send while idle releases the held queue");
        };
        assert_eq!(next.content, "second");
        assert!(!queue.held());
        assert_eq!(contents(&queue), ["third"]);
    }

    #[test]
    fn a_stop_with_nothing_left_leaves_the_queue_idle() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        queue.stop(false);
        assert_eq!(queue.finish(), Finished::Idle);
        assert!(!queue.held());
    }

    #[test]
    fn interrupting_moves_the_message_to_the_front_and_asks_for_a_cancel() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        queue.submit(queued("second"), false);
        let third = queued("third");
        queue.submit(third.clone(), false);

        assert_eq!(queue.interrupt(third.id, false), Interrupted::Cancel);
        assert_eq!(contents(&queue), ["third", "second"]);
        let Finished::Next(next) = queue.finish() else {
            panic!("an interrupted turn is not a held one");
        };
        assert_eq!(next.content, "third");
    }

    #[test]
    fn interrupting_after_a_stop_does_not_hold_the_message() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        let second = queued("second");
        queue.submit(second.clone(), false);
        queue.stop(false);
        assert_eq!(queue.interrupt(second.id, false), Interrupted::Cancel);
        assert_eq!(queue.finish(), Finished::Next(second));
    }

    #[test]
    fn a_held_message_sends_at_once_when_asked() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        let second = queued("second");
        queue.submit(second.clone(), false);
        queue.stop(false);
        queue.finish();

        assert_eq!(
            queue.interrupt(second.id, false),
            Interrupted::Start(second)
        );
        assert!(!queue.held());
        assert_eq!(queue.interrupt(Uuid::new_v4(), false), Interrupted::Missing);
    }

    #[test]
    fn a_queued_message_can_be_edited_or_removed_before_it_goes() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        let second = queued("second");
        queue.submit(second.clone(), false);
        let third = queued("third");
        queue.submit(third.clone(), false);

        assert!(queue.edit(second.id, "second, edited".into()));
        assert!(queue.remove(third.id));
        assert!(!queue.remove(third.id));
        assert!(!queue.edit(third.id, "gone".into()));
        assert_eq!(contents(&queue), ["second, edited"]);
    }

    #[tokio::test]
    async fn the_queue_drains_into_turns_when_each_turn_ends() {
        let chat_id = Uuid::new_v4();
        let first = with(chat_id, |queue| queue.submit(queued("first"), false)).unwrap();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let announced = Arc::new(Mutex::new(Vec::new()));

        let turns = ran.clone();
        let told = announced.clone();
        drain(
            chat_id,
            first,
            move |message: Queued| {
                let turns = turns.clone();
                async move {
                    if message.content == "first" {
                        with(chat_id, |queue| {
                            queue.submit(queued("second"), false);
                            queue.submit(queued("third"), false);
                        });
                    }
                    turns.lock().unwrap().push(message.content);
                }
            },
            move |started| {
                let told = told.clone();
                async move { told.lock().unwrap().push(started.is_some()) }
            },
        )
        .await;

        assert_eq!(*ran.lock().unwrap(), ["first", "second", "third"]);
        assert_eq!(*announced.lock().unwrap(), [true, true]);
        assert!(snapshot(chat_id).is_none());
        assert!(!QUEUES.contains_key(&chat_id), "an idle queue is forgotten");
    }

    #[tokio::test]
    async fn a_stopped_turn_leaves_the_queue_for_the_user() {
        let chat_id = Uuid::new_v4();
        let first = with(chat_id, |queue| queue.submit(queued("first"), false)).unwrap();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let announced = Arc::new(Mutex::new(Vec::new()));

        let turns = ran.clone();
        let told = announced.clone();
        drain(
            chat_id,
            first,
            move |message: Queued| {
                let turns = turns.clone();
                async move {
                    with(chat_id, |queue| {
                        queue.submit(queued("second"), false);
                        queue.stop(false);
                    });
                    turns.lock().unwrap().push(message.content);
                }
            },
            move |started| {
                let told = told.clone();
                async move { told.lock().unwrap().push(started) }
            },
        )
        .await;

        assert_eq!(*ran.lock().unwrap(), ["first"]);
        assert_eq!(*announced.lock().unwrap(), [None]);
        let (messages, held) = snapshot(chat_id).expect("the held message stays queued");
        assert!(held);
        assert_eq!(messages[0].content, "second");
    }
}
