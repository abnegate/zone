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
    /// The turn runs as whoever last wrote the message, not whoever is
    /// connected when it starts.
    #[serde(skip)]
    pub author: Uuid,
}

/// A message leaving the queue, and the id its turn is registered under.
#[derive(Debug, PartialEq)]
pub struct Started {
    pub message: Queued,
    pub turn: Uuid,
}

#[derive(Debug, PartialEq)]
pub enum Finished {
    Next(Started),
    Held,
    Idle,
}

#[derive(Debug, PartialEq)]
pub enum Interrupted {
    /// The message is next; the running turn has to be stopped for it to start.
    Cancel,
    Start(Started),
    Missing,
}

#[derive(Debug)]
struct Current {
    message: Queued,
    turn: Uuid,
    cancelled: bool,
}

#[derive(Debug, Default)]
pub struct Queue {
    messages: VecDeque<Queued>,
    current: Option<Current>,
    stopping: bool,
    interrupting: bool,
    held: bool,
}

impl Queue {
    /// Queue a message and start it if nothing is running. `busy` is a turn
    /// this queue did not start, such as an automation's.
    pub fn submit(&mut self, mut message: Queued, busy: bool) -> Option<Started> {
        if self.contains(message.id) {
            message.id = Uuid::new_v4();
        }
        self.messages.push_back(message);
        if self.current.is_some() || busy {
            return None;
        }
        self.held = false;
        self.start()
    }

    /// The turn this queue started has ended. A turn the user stopped holds
    /// what is left, so Stop does not immediately start the next message. A
    /// turn cancelled before its message was saved goes back in the queue.
    pub fn finish(&mut self, saved: bool) -> Finished {
        if let Some(current) = self.current.take()
            && current.cancelled
            && !saved
        {
            let at = usize::from(self.interrupting).min(self.messages.len());
            self.messages.insert(at, current.message);
        }
        self.interrupting = false;
        if std::mem::take(&mut self.stopping) || self.held {
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
        if self.current.is_some() {
            return Finished::Idle;
        }
        self.finish(true)
    }

    pub fn stop(&mut self, busy: bool) {
        if let Some(current) = self.current.as_mut() {
            current.cancelled = true;
            self.stopping = true;
        } else if busy {
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
        if self.current.is_none() && !busy {
            return self
                .start()
                .map_or(Interrupted::Missing, Interrupted::Start);
        }
        if let Some(current) = self.current.as_mut() {
            current.cancelled = true;
        }
        self.stopping = false;
        self.interrupting = true;
        Interrupted::Cancel
    }

    pub fn edit(&mut self, id: Uuid, content: String, author: Uuid) -> bool {
        let Some(message) = self.messages.iter_mut().find(|message| message.id == id) else {
            return false;
        };
        message.content = content;
        message.author = author;
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

    fn start(&mut self) -> Option<Started> {
        let message = self.messages.pop_front()?;
        let turn = Uuid::new_v4();
        self.current = Some(Current {
            message: message.clone(),
            turn,
            cancelled: false,
        });
        self.stopping = false;
        Some(Started { message, turn })
    }

    /// The draining task died mid-turn. Nothing will finish that turn, so hold
    /// what is left rather than leave every later message waiting on it.
    fn abandon(&mut self) {
        self.current = None;
        self.stopping = false;
        self.interrupting = false;
        self.held = !self.messages.is_empty();
    }

    fn contains(&self, id: Uuid) -> bool {
        self.messages.iter().any(|message| message.id == id)
            || self
                .current
                .as_ref()
                .is_some_and(|current| current.message.id == id)
    }

    fn is_idle(&self) -> bool {
        self.current.is_none() && self.messages.is_empty()
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

struct Abandon(Uuid);

impl Drop for Abandon {
    fn drop(&mut self) {
        with(self.0, Queue::abandon);
    }
}

enum Step<Ran> {
    Run(Queued, Ran),
    Held,
    Idle,
}

/// Run `first`, then every message queued behind it, until the queue is empty
/// or held. `turn` is called under the queue's lock, so a turn is registered
/// for cancelling before anything else can see it has started. `announce` is
/// told which message left the queue to start, or `None` when the queue came
/// to rest holding messages.
pub async fn drain<Turn, Ran, Announce, Announced>(
    chat_id: Uuid,
    first: Ran,
    mut turn: Turn,
    mut announce: Announce,
) where
    Turn: FnMut(Started) -> Ran,
    Ran: Future<Output = bool>,
    Announce: FnMut(Option<Queued>) -> Announced,
    Announced: Future<Output = ()>,
{
    let abandon = Abandon(chat_id);
    let mut running = first;
    loop {
        let saved = running.await;
        let step = with(chat_id, |queue| match queue.finish(saved) {
            Finished::Next(started) => Step::Run(started.message.clone(), turn(started)),
            Finished::Held => Step::Held,
            Finished::Idle => Step::Idle,
        });
        match step {
            Step::Run(message, next) => {
                running = next;
                announce(Some(message)).await;
            }
            Step::Held => {
                std::mem::forget(abandon);
                announce(None).await;
                return;
            }
            Step::Idle => {
                std::mem::forget(abandon);
                return;
            }
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

    fn next(finished: Finished) -> String {
        match finished {
            Finished::Next(started) => started.message.content,
            other => panic!("expected the next message to start, got {other:?}"),
        }
    }

    #[test]
    fn a_message_starts_at_once_when_nothing_is_running() {
        let mut queue = Queue::default();
        let first = queued("first");
        assert_eq!(queue.submit(first.clone(), false).unwrap().message, first);
        assert!(queue.messages().is_empty());
    }

    #[test]
    fn a_message_sent_during_a_turn_waits_and_runs_next_in_order() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        assert_eq!(queue.submit(queued("second"), false), None);
        assert_eq!(queue.submit(queued("third"), false), None);
        assert_eq!(contents(&queue), ["second", "third"]);

        assert_eq!(next(queue.finish(true)), "second");
        assert_eq!(next(queue.finish(true)), "third");
        assert_eq!(queue.finish(true), Finished::Idle);
    }

    #[test]
    fn each_turn_gets_its_own_registration() {
        let mut queue = Queue::default();
        let Some(first) = queue.submit(queued("first"), false) else {
            panic!("an idle queue starts the message");
        };
        queue.submit(queued("second"), false);
        let Finished::Next(second) = queue.finish(true) else {
            panic!("the queue drains when the turn ends");
        };
        assert_ne!(first.turn, second.turn);
    }

    #[test]
    fn a_duplicate_id_is_replaced_rather_than_shadowing_another_message() {
        let mut queue = Queue::default();
        let first = queued("first");
        queue.submit(first.clone(), false);
        queue.submit(
            Queued {
                content: "again".into(),
                ..first.clone()
            },
            false,
        );
        let second = queued("second");
        queue.submit(second.clone(), false);
        queue.submit(
            Queued {
                content: "twice".into(),
                ..second.clone()
            },
            false,
        );

        let ids: Vec<Uuid> = queue.messages().iter().map(|message| message.id).collect();
        assert_eq!(ids.len(), 3);
        assert!(!ids.contains(&first.id), "the running message keeps its id");
        assert_eq!(ids.iter().filter(|id| **id == second.id).count(), 1);
    }

    #[test]
    fn a_turn_someone_else_started_makes_a_message_wait() {
        let mut queue = Queue::default();
        assert_eq!(queue.submit(queued("typed"), true), None);
        assert_eq!(next(queue.release()), "typed");
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
        assert_eq!(queue.finish(true), Finished::Held);
        assert!(queue.held());
        assert_eq!(contents(&queue), ["second"]);

        let Some(next) = queue.submit(queued("third"), false) else {
            panic!("a send while idle releases the held queue");
        };
        assert_eq!(next.message.content, "second");
        assert!(!queue.held());
        assert_eq!(contents(&queue), ["third"]);
    }

    #[test]
    fn a_stop_with_nothing_left_leaves_the_queue_idle() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        queue.stop(false);
        assert_eq!(queue.finish(true), Finished::Idle);
        assert!(!queue.held());
    }

    #[test]
    fn a_turn_stopped_before_its_message_was_saved_is_held_rather_than_lost() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        queue.submit(queued("second"), false);
        queue.stop(false);
        assert_eq!(queue.finish(false), Finished::Held);
        assert_eq!(contents(&queue), ["first", "second"]);
    }

    #[test]
    fn a_turn_that_failed_before_saving_is_not_retried() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        queue.submit(queued("second"), false);
        assert_eq!(next(queue.finish(false)), "second");
        assert!(queue.messages().is_empty());
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
        assert_eq!(next(queue.finish(true)), "third");
    }

    #[test]
    fn a_turn_interrupted_before_saving_goes_back_behind_the_interrupting_message() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        let second = queued("second");
        queue.submit(second.clone(), false);

        assert_eq!(queue.interrupt(second.id, false), Interrupted::Cancel);
        assert_eq!(next(queue.finish(false)), "second");
        assert_eq!(contents(&queue), ["first"]);
    }

    #[test]
    fn interrupting_after_a_stop_does_not_hold_the_message() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        let second = queued("second");
        queue.submit(second.clone(), false);
        queue.stop(false);
        assert_eq!(queue.interrupt(second.id, false), Interrupted::Cancel);
        assert_eq!(next(queue.finish(true)), "second");
    }

    #[test]
    fn a_held_message_sends_at_once_when_asked() {
        let mut queue = Queue::default();
        queue.submit(queued("first"), false);
        let second = queued("second");
        queue.submit(second.clone(), false);
        queue.stop(false);
        queue.finish(true);

        let Interrupted::Start(started) = queue.interrupt(second.id, false) else {
            panic!("nothing is running, so the message starts");
        };
        assert_eq!(started.message, second);
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
        let editor = Uuid::new_v4();

        assert!(queue.edit(second.id, "second, edited".into(), editor));
        assert!(queue.remove(third.id));
        assert!(!queue.remove(third.id));
        assert!(!queue.edit(third.id, "gone".into(), editor));
        assert_eq!(contents(&queue), ["second, edited"]);
        assert_eq!(
            queue.messages()[0].author,
            editor,
            "the turn runs as its editor"
        );
    }

    async fn run(
        chat_id: Uuid,
        during: impl Fn(&Queued) + Send + Sync + 'static,
    ) -> (Vec<String>, Vec<Option<String>>) {
        let first = with(chat_id, |queue| queue.submit(queued("first"), false)).unwrap();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let announced = Arc::new(Mutex::new(Vec::new()));
        let during = Arc::new(during);
        let turn = {
            let ran = ran.clone();
            move |started: Started| {
                let ran = ran.clone();
                let during = during.clone();
                async move {
                    during(&started.message);
                    ran.lock().unwrap().push(started.message.content);
                    true
                }
            }
        };
        let told = announced.clone();
        drain(
            chat_id,
            turn(first),
            turn,
            move |started: Option<Queued>| {
                let told = told.clone();
                async move {
                    told.lock()
                        .unwrap()
                        .push(started.map(|message| message.content))
                }
            },
        )
        .await;
        let ran = ran.lock().unwrap().clone();
        let announced = announced.lock().unwrap().clone();
        (ran, announced)
    }

    #[tokio::test]
    async fn the_queue_drains_into_turns_when_each_turn_ends() {
        let chat_id = Uuid::new_v4();
        let (ran, announced) = run(chat_id, move |message| {
            if message.content == "first" {
                with(chat_id, |queue| {
                    queue.submit(queued("second"), false);
                    queue.submit(queued("third"), false);
                });
            }
        })
        .await;

        assert_eq!(ran, ["first", "second", "third"]);
        assert_eq!(
            announced,
            [Some("second".to_string()), Some("third".to_string())]
        );
        assert!(snapshot(chat_id).is_none());
        assert!(!QUEUES.contains_key(&chat_id), "an idle queue is forgotten");
    }

    #[tokio::test]
    async fn a_stopped_turn_leaves_the_queue_for_the_user() {
        let chat_id = Uuid::new_v4();
        let (ran, announced) = run(chat_id, move |_| {
            with(chat_id, |queue| {
                queue.submit(queued("second"), false);
                queue.stop(false);
            });
        })
        .await;

        assert_eq!(ran, ["first"]);
        assert_eq!(announced, [None]);
        let (messages, held) = snapshot(chat_id).expect("the held message stays queued");
        assert!(held);
        assert_eq!(messages[0].content, "second");
    }

    #[tokio::test]
    async fn a_turn_that_dies_does_not_leave_the_queue_waiting_on_it() {
        let chat_id = Uuid::new_v4();
        let first = with(chat_id, |queue| queue.submit(queued("first"), false)).unwrap();
        with(chat_id, |queue| queue.submit(queued("second"), false));
        let turn = |started: Started| async move {
            assert_ne!(started.message.content, "first", "the turn died");
            true
        };
        let died = tokio::spawn(drain(
            chat_id,
            turn(first),
            turn,
            |_: Option<Queued>| async {},
        ))
        .await;
        assert!(died.is_err());

        let (messages, held) = snapshot(chat_id).expect("the waiting message is kept");
        assert!(held);
        assert_eq!(messages[0].content, "second");
        assert!(
            with(chat_id, |queue| queue.submit(queued("third"), false)).is_some(),
            "the next send starts rather than queueing behind a dead turn"
        );
    }
}
