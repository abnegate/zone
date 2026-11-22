//! Relays what the other replicas cannot see in this process's registries:
//! answers to task-run questions, and chat title and action-card updates.

mod action;
mod answered;
mod answering;
mod local;
mod registry;
mod relay;
mod title;

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::agent::question::Answer;
use crate::cluster::Bus;

pub use local::Local;
pub use registry::Registry;
pub use relay::Relay;

static RELAY: OnceCell<Relay<Local>> = OnceCell::const_new();

/// Starts this process's bridges on `bus`. Only the first call starts them.
pub async fn serve(bus: &Bus) {
    RELAY
        .get_or_init(|| Relay::start(bus.clone(), Arc::new(Local)))
        .await;
}

/// Delivers a chat's new title here and on every other replica.
pub fn title(chat: Uuid, title: String) {
    match RELAY.get() {
        Some(relay) => relay.title(chat, title),
        None => Local.title(chat, title),
    }
}

/// Delivers a chat's new action card here and on every other replica.
pub fn action(chat: Uuid, message: Value) {
    match RELAY.get() {
        Some(relay) => relay.action(chat, message),
        None => Local.action(chat, message),
    }
}

/// Resolves the run waiting on `run`, wherever it waits. Returns whether
/// anything was waiting.
pub async fn answer(run: Uuid, answers: Vec<Answer>) -> bool {
    match RELAY.get() {
        Some(relay) => relay.answer(run, answers).await,
        None => Local.answer(run, answers),
    }
}
