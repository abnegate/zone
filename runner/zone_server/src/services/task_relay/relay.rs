use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::{CancellationToken, DropGuard};
use uuid::Uuid;

use super::action::Action;
use super::answered::Answered;
use super::answering::Answering;
use super::registry::Registry;
use super::title::Title;
use crate::agent::question::Answer;
use crate::cluster::{Bus, Envelope, Kind, Subscription, Topic};

/// How long an answer waits for the replica running the run to say it took it.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// A registry joined to the other replicas over the bus: what happens here is
/// delivered here and published, and what another replica published is
/// delivered here and never published again. The bus never hands a replica
/// its own envelopes, so nothing is delivered twice.
pub struct Relay<R: Registry> {
    bus: Bus,
    registry: Arc<R>,
    _bridges: DropGuard,
}

impl<R: Registry> Relay<R> {
    /// Subscribes the bridges before returning, so nothing published after
    /// this returns can miss them. Dropping the relay stops them.
    pub async fn start(bus: Bus, registry: Arc<R>) -> Self {
        let stop = CancellationToken::new();
        let runs = bus.subscribe(Topic::TaskRuns).await;
        let titles = bus.subscribe(Topic::Titles).await;
        let actions = bus.subscribe(Topic::Actions).await;
        let answering = {
            let bus = bus.clone();
            let registry = Arc::clone(&registry);
            move |envelope: Envelope| take(&bus, &*registry, &envelope)
        };
        let titling = {
            let registry = Arc::clone(&registry);
            move |envelope: Envelope| retitle(&*registry, &envelope)
        };
        let carding = {
            let registry = Arc::clone(&registry);
            move |envelope: Envelope| card(&*registry, &envelope)
        };
        tokio::spawn(bridge(runs, stop.clone(), answering));
        tokio::spawn(bridge(titles, stop.clone(), titling));
        tokio::spawn(bridge(actions, stop.clone(), carding));
        Self {
            bus,
            registry,
            _bridges: stop.drop_guard(),
        }
    }

    pub fn title(&self, chat: Uuid, title: String) {
        let update = Title { chat, title };
        self.bus.publish(Topic::Titles, &update);
        self.registry.title(update.chat, update.title);
    }

    pub fn action(&self, chat: Uuid, message: Value) {
        let update = Action { chat, message };
        self.bus.publish(Topic::Actions, &update);
        self.registry.action(update.chat, update.message);
    }

    /// Resolves the run here when it waits here, and otherwise asks the
    /// other replicas. Returns whether any of them had it waiting.
    pub async fn answer(&self, run: Uuid, answers: Vec<Answer>) -> bool {
        if self.registry.answer(run, answers.clone()) {
            return true;
        }
        if !self.bus.connected() {
            return false;
        }
        self.bus
            .request(Topic::TaskRuns, &Answering { run, answers }, ANSWER_TIMEOUT)
            .await
            .and_then(|reply| reply.payload::<Answered>().ok())
            .is_some_and(|answered| answered.run == run)
    }
}

async fn bridge(
    mut subscription: Subscription,
    stop: CancellationToken,
    handle: impl Fn(Envelope) + Send + 'static,
) {
    loop {
        let received = tokio::select! {
            () = stop.cancelled() => return,
            received = subscription.recv() => received,
        };
        match received {
            Ok(envelope) => handle(envelope),
            Err(RecvError::Lagged(missed)) => tracing::warn!(
                topic = ?subscription.topic(),
                missed,
                "Missed updates relayed by other replicas"
            ),
            Err(RecvError::Closed) => return,
        }
    }
}

fn take(bus: &Bus, registry: &impl Registry, envelope: &Envelope) {
    if envelope.kind() != Kind::Request {
        return;
    }
    let Some(Answering { run, answers }) = decode(envelope) else {
        return;
    };
    if registry.answer(run, answers) {
        bus.reply(envelope, &Answered { run });
    }
}

fn retitle(registry: &impl Registry, envelope: &Envelope) {
    if let Some(Title { chat, title }) = decode(envelope) {
        registry.title(chat, title);
    }
}

fn card(registry: &impl Registry, envelope: &Envelope) {
    if let Some(Action { chat, message }) = decode(envelope) {
        registry.action(chat, message);
    }
}

fn decode<T: serde::de::DeserializeOwned>(envelope: &Envelope) -> Option<T> {
    envelope
        .payload()
        .inspect_err(|error| {
            tracing::debug!(topic = ?envelope.topic(), %error, "Dropped a malformed relayed update");
        })
        .ok()
}
