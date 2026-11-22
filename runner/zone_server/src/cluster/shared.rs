use std::sync::Arc;

use bytes::Bytes;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use tokio::sync::{broadcast, oneshot, watch};
use tokio_util::sync::DropGuard;
use uuid::Uuid;

use super::channel::Channel;
use super::envelope::Envelope;
use super::kind::Kind;
use super::namespace::Namespace;
use super::packet::Packet;
use super::signer::Signer;
use super::status::Status;
use super::topic::Topic;
use super::transport::Transport;

const CAPACITY: usize = 1024;

/// The state every handle on one bus shares.
pub struct Shared {
    pub instance: Uuid,
    pub namespace: Namespace,
    pub signer: Signer,
    /// The channel of this instance's own topic, which replies arrive on and
    /// which stays subscribed for the bus's whole life.
    pub home: Arc<str>,
    pub channels: DashMap<Arc<str>, Channel>,
    pub pending: DashMap<Uuid, oneshot::Sender<Envelope>>,
    pub transport: Transport,
    pub status: Arc<Status>,
    pub stop: DropGuard,
}

impl Shared {
    pub fn send(
        &self,
        topic: Topic,
        kind: Kind,
        correlation: Option<Uuid>,
        header: Bytes,
        payload: Bytes,
    ) {
        let envelope = Envelope::new(topic, kind, self.instance, correlation, header, payload);
        if topic == Topic::Instance(self.instance) {
            self.deliver(&self.home, envelope);
            return;
        }
        if !self.transport.reaches_peers() {
            return;
        }
        let channel: Arc<str> = topic.channel(&self.namespace).into();
        let bytes = envelope.encode(&channel, &self.signer);
        self.transport.send(Packet { channel, bytes });
    }

    /// Checks an envelope that arrived on `channel` and hands it to whoever
    /// waits for it. An envelope this instance sent comes back only on an
    /// instance topic; anything that fails its signature is dropped.
    pub fn receive(&self, channel: &str, bytes: Bytes) {
        let topic = if *channel == *self.home {
            Topic::Instance(self.instance)
        } else {
            match self.channels.get(channel) {
                Some(subscribed) => subscribed.topic,
                None => return,
            }
        };
        let unverified = match Envelope::decode(topic, bytes) {
            Ok(unverified) => unverified,
            Err(rejection) => {
                tracing::debug!(channel, %rejection, "Dropped a bus envelope");
                return;
            }
        };
        if unverified.claimed().origin() == self.instance && !matches!(topic, Topic::Instance(_)) {
            return;
        }
        match unverified.verify(channel, &self.signer) {
            Ok(envelope) => self.deliver(channel, envelope),
            Err(rejection) => tracing::debug!(channel, %rejection, "Dropped a bus envelope"),
        }
    }

    fn deliver(&self, channel: &str, envelope: Envelope) {
        if envelope.kind() == Kind::Reply && *channel == *self.home {
            if let Some((_, waiter)) = envelope
                .correlation()
                .and_then(|correlation| self.pending.remove(&correlation))
            {
                let _ = waiter.send(envelope);
            }
            return;
        }
        if let Some(subscribed) = self.channels.get(channel) {
            let _ = subscribed.sender.send(envelope);
        }
    }

    /// Adds a subscriber to `topic`, subscribing the transport for the first.
    pub fn join(
        &self,
        topic: Topic,
    ) -> (
        Arc<str>,
        broadcast::Receiver<Envelope>,
        watch::Receiver<bool>,
    ) {
        let channel: Arc<str> = topic.channel(&self.namespace).into();
        match self.channels.entry(Arc::clone(&channel)) {
            Entry::Occupied(mut occupied) => {
                let subscribed = occupied.get_mut();
                subscribed.subscribers += 1;
                (
                    channel,
                    subscribed.sender.subscribe(),
                    subscribed.confirmed.clone(),
                )
            }
            Entry::Vacant(vacant) => {
                let (sender, receiver) = broadcast::channel(CAPACITY);
                let (confirm, confirmed) = watch::channel(false);
                if channel == self.home {
                    confirm.send_replace(true);
                } else {
                    self.transport.subscribe(Arc::clone(&channel), confirm);
                }
                vacant.insert(Channel {
                    topic,
                    sender,
                    subscribers: 1,
                    confirmed: confirmed.clone(),
                });
                (channel, receiver, confirmed)
            }
        }
    }

    /// Removes a subscriber, unsubscribing the transport after the last.
    pub fn leave(&self, channel: &Arc<str>) {
        let Entry::Occupied(mut occupied) = self.channels.entry(Arc::clone(channel)) else {
            return;
        };
        let subscribed = occupied.get_mut();
        subscribed.subscribers -= 1;
        if subscribed.subscribers > 0 {
            return;
        }
        occupied.remove();
        if *channel != self.home {
            self.transport.unsubscribe(Arc::clone(channel));
        }
    }

    /// Every channel the pub/sub connection must listen on.
    pub fn channels(&self) -> Vec<String> {
        let mut channels: Vec<String> = self
            .channels
            .iter()
            .map(|subscribed| subscribed.key().to_string())
            .filter(|channel| **channel != *self.home)
            .collect();
        channels.push(self.home.to_string());
        channels
    }
}
