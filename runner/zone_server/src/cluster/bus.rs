use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use abnegate_secret::MasterKey;
use bytes::Bytes;
use dashmap::DashMap;
use metrics::Gauge;
use redis::aio::ConnectionManager;
use serde::Serialize;
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::clock::SKEW;
use super::envelope::Envelope;
use super::kind::Kind;
use super::loopback::Loopback;
use super::namespace::Namespace;
use super::pending::Pending;
use super::sealer::Sealer;
use super::seen::Seen;
use super::shared::Shared;
use super::status::Status;
use super::subscription::Subscription;
use super::topic::Topic;
use super::transport::Transport;
use super::valkey::{LOG_INTERVAL, Valkey};

const CONFIRMATION: Duration = Duration::from_secs(2);

/// The sealed, best-effort bus replicas relay live state over: Valkey pub/sub
/// when it is configured, or an in-process loopback when it is not. Durable
/// state stays in Postgres; nothing here is retried or persisted.
#[derive(Clone)]
pub struct Bus {
    shared: Arc<Shared>,
}

impl Bus {
    /// A bus nothing else hears: what a single node runs on.
    pub fn loopback(master: &MasterKey) -> Self {
        Self::attach(&Loopback::new(), master)
    }

    /// A bus on an in-process hub, which every other bus attached to the same
    /// hub hears as if it were another replica.
    pub fn attach(hub: &Loopback, master: &MasterKey) -> Self {
        Self::build(master, None, |shared, stop| {
            hub.attach(shared, stop);
            Transport::Loopback(hub.clone())
        })
    }

    /// A bus on the Valkey at `url`. It never fails: until Valkey answers, it
    /// keeps retrying, delivers only to this instance and reports 0 on `gauge`.
    /// A `url` that cannot be used at all leaves the bus on the loopback,
    /// logging an error every minute.
    pub fn connect(url: &str, master: &MasterKey, gauge: Gauge) -> Self {
        let instance = Uuid::new_v4();
        let (client, manager) = match Valkey::client(url, instance) {
            Ok(connections) => connections,
            Err(error) => return Self::stranded(master, gauge, error),
        };
        let status = Arc::new(Status::new(Some(gauge)));
        Self::assemble(master, instance, Arc::clone(&status), |shared, stop| {
            Transport::Valkey(Valkey::start(client, manager, shared, status, stop))
        })
    }

    fn stranded(master: &MasterKey, gauge: Gauge, error: redis::RedisError) -> Self {
        Self::build(master, Some(gauge), |shared, stop| {
            let hub = Loopback::new();
            hub.attach(shared, stop.clone());
            tokio::spawn(remind(error.to_string(), stop));
            Transport::Loopback(hub)
        })
    }

    fn build(
        master: &MasterKey,
        gauge: Option<Gauge>,
        transport: impl FnOnce(Weak<Shared>, CancellationToken) -> Transport,
    ) -> Self {
        Self::assemble(
            master,
            Uuid::new_v4(),
            Arc::new(Status::new(gauge)),
            transport,
        )
    }

    fn assemble(
        master: &MasterKey,
        instance: Uuid,
        status: Arc<Status>,
        transport: impl FnOnce(Weak<Shared>, CancellationToken) -> Transport,
    ) -> Self {
        let namespace = Namespace::derive(master);
        let home: Arc<str> = Topic::Instance(instance).channel(&namespace).into();
        let stop = CancellationToken::new();
        let shared = Arc::new_cyclic(|shared| Shared {
            instance,
            namespace,
            sealer: Sealer::derive(master),
            home,
            channels: DashMap::new(),
            pending: DashMap::new(),
            seen: Mutex::new(Seen::new(Kind::Request.lifetime() + SKEW)),
            transport: transport(shared.clone(), stop.clone()),
            status,
            stop: stop.drop_guard(),
        });
        Self { shared }
    }

    /// This process's id on the bus, which its replies are addressed to.
    pub fn instance(&self) -> Uuid {
        self.shared.instance
    }

    /// Whether the pub/sub connection is subscribed. Always false on the loopback.
    pub fn connected(&self) -> bool {
        self.shared.status.connected()
    }

    pub fn connection(&self) -> watch::Receiver<bool> {
        self.shared.status.watch()
    }

    /// The namespace every key and channel of this cluster lives in.
    pub fn namespace(&self) -> &Namespace {
        &self.shared.namespace
    }

    /// The Valkey connection the bus publishes on, for state the replicas
    /// share outside pub/sub. None on the loopback.
    pub fn valkey(&self) -> Option<ConnectionManager> {
        self.shared.transport.valkey().cloned()
    }

    pub fn publish(&self, topic: Topic, value: &impl Serialize) {
        if let Some(payload) = json(value) {
            self.shared
                .send(topic, Kind::Message, None, Bytes::new(), payload);
        }
    }

    /// Publishes bytes that are already serialized, with a header receivers
    /// read through [`Envelope::header`] without parsing the payload.
    pub fn publish_raw(&self, topic: Topic, header: &impl Serialize, payload: Bytes) {
        if let Some(header) = json(header) {
            self.shared
                .send(topic, Kind::Message, None, header, payload);
        }
    }

    /// Subscribes to `topic`. Over Valkey it waits, briefly, until the
    /// subscription is live, so whatever the caller publishes or asks for
    /// next cannot race ahead of it.
    pub async fn subscribe(&self, topic: Topic) -> Subscription {
        let (channel, receiver, mut confirmed) = self.shared.join(topic);
        let subscription = Subscription::new(topic, channel, receiver, Arc::clone(&self.shared));
        if self.connected() {
            let _ = tokio::time::timeout(CONFIRMATION, confirmed.wait_for(|live| *live)).await;
        }
        subscription
    }

    /// Asks whoever answers `topic`, waiting up to `timeout` for the first
    /// reply. None when nobody answered in time.
    pub async fn request(
        &self,
        topic: Topic,
        value: &impl Serialize,
        timeout: Duration,
    ) -> Option<Envelope> {
        let payload = json(value)?;
        let correlation = Uuid::new_v4();
        let (waiter, reply) = oneshot::channel();
        self.shared.pending.insert(correlation, waiter);
        let _pending = Pending::new(correlation, Arc::clone(&self.shared));
        self.shared.send(
            topic,
            Kind::Request,
            Some(correlation),
            Bytes::new(),
            payload,
        );
        tokio::time::timeout(timeout, reply).await.ok()?.ok()
    }

    /// Answers `request` on its sender's instance topic.
    pub fn reply(&self, request: &Envelope, value: &impl Serialize) {
        let (Kind::Request, Some(correlation)) = (request.kind(), request.correlation()) else {
            tracing::debug!(kind = ?request.kind(), "Ignored a reply to an envelope that is not a request");
            return;
        };
        if let Some(payload) = json(value) {
            self.shared.send(
                Topic::Instance(request.origin()),
                Kind::Reply,
                Some(correlation),
                Bytes::new(),
                payload,
            );
        }
    }
}

fn json(value: &impl Serialize) -> Option<Bytes> {
    match serde_json::to_vec(value) {
        Ok(bytes) => Some(Bytes::from(bytes)),
        Err(error) => {
            tracing::error!(%error, "Could not serialize a bus envelope");
            None
        }
    }
}

async fn remind(reason: String, stop: CancellationToken) {
    let mut reminder = tokio::time::interval(LOG_INTERVAL);
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            _ = reminder.tick() => tracing::error!(
                %reason,
                "REDIS_URL is set but the cluster bus cannot use it, so it runs on the in-process loopback; replicas cannot relay chats, answers or titles to each other"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use super::*;
    use crate::cluster::clock::{self, milliseconds};
    use crate::cluster::packet::Packet;
    use crate::secret;
    use serde::Deserialize;
    use tokio::sync::broadcast;
    use tokio::sync::broadcast::error::TryRecvError;

    const PASSPHRASE: &str = "a-cluster-passphrase-of-at-least-32-bytes";
    const OTHER_PASSPHRASE: &str = "another-cluster-passphrase-of-32-bytes!";
    const WAIT: Duration = Duration::from_secs(2);
    const SILENCE: Duration = Duration::from_millis(200);
    const MARKER: &str = "a-secret-the-wire-must-not-show";

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Title {
        chat: Uuid,
        title: String,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Frame {
        sequence: u64,
        terminator: bool,
    }

    #[derive(Default)]
    struct Reading {
        bits: AtomicU64,
        written: AtomicBool,
    }

    impl Reading {
        fn value(&self) -> f64 {
            f64::from_bits(self.bits.load(Ordering::SeqCst))
        }

        fn written(&self) -> bool {
            self.written.load(Ordering::SeqCst)
        }
    }

    impl metrics::GaugeFn for Reading {
        fn increment(&self, value: f64) {
            self.set(self.value() + value);
        }

        fn decrement(&self, value: f64) {
            self.set(self.value() - value);
        }

        fn set(&self, value: f64) {
            self.bits.store(value.to_bits(), Ordering::SeqCst);
            self.written.store(true, Ordering::SeqCst);
        }
    }

    fn master() -> Arc<MasterKey> {
        secret::derive(PASSPHRASE).unwrap()
    }

    fn pair() -> (Bus, Bus) {
        let hub = Loopback::new();
        (Bus::attach(&hub, &master()), Bus::attach(&hub, &master()))
    }

    fn title() -> Title {
        Title {
            chat: Uuid::new_v4(),
            title: "Quarterly plan".to_string(),
        }
    }

    async fn next(subscription: &mut Subscription) -> Envelope {
        tokio::time::timeout(WAIT, subscription.recv())
            .await
            .expect("an envelope arrives")
            .expect("the subscription is open")
    }

    async fn silent(subscription: &mut Subscription) {
        tokio::time::sleep(SILENCE).await;
        assert!(matches!(subscription.try_recv(), Err(TryRecvError::Empty)));
    }

    async fn crossing(wire: &mut broadcast::Receiver<Packet>) -> Packet {
        tokio::time::timeout(WAIT, wire.recv())
            .await
            .expect("a packet crosses the hub")
            .expect("the hub is open")
    }

    fn shows(bytes: &[u8], text: &str) -> bool {
        bytes
            .windows(text.len())
            .any(|window| window == text.as_bytes())
    }

    #[tokio::test]
    async fn a_chat_frame_crosses_the_wire_sealed() {
        let hub = Loopback::new();
        let mut wire = hub.tap();
        let sender = Bus::attach(&hub, &master());
        let receiver = Bus::attach(&hub, &master());
        let chat = Uuid::new_v4();
        let mut frames = receiver.subscribe(Topic::ChatFrames(chat)).await;
        let header = format!("{MARKER}-in-the-header");
        let payload = Bytes::from(format!(
            r#"{{"type":"chunk","content":"{MARKER}-in-the-payload"}}"#
        ));

        sender.publish_raw(Topic::ChatFrames(chat), &header, payload.clone());

        let packet = crossing(&mut wire).await;
        assert!(!shows(&packet.bytes, MARKER), "{:?}", packet.bytes);
        let envelope = next(&mut frames).await;
        assert_eq!(envelope.bytes(), &payload);
        assert_eq!(envelope.header::<String>().unwrap(), header);
    }

    #[tokio::test]
    async fn a_request_delivered_twice_is_heard_once() {
        let hub = Loopback::new();
        let mut wire = hub.tap();
        let asker = Bus::attach(&hub, &master());
        let answerer = Bus::attach(&hub, &master());
        let chat = Uuid::new_v4();
        let mut control = answerer.subscribe(Topic::ChatControl(chat)).await;

        let unanswered = asker
            .request(Topic::ChatControl(chat), &title(), SILENCE)
            .await;
        let captured = crossing(&mut wire).await;
        hub.send(captured);

        assert!(unanswered.is_none());
        assert_eq!(next(&mut control).await.kind(), Kind::Request);
        silent(&mut control).await;
    }

    /// A genuine envelope sealed by a peer of this cluster at `issued_at`.
    fn sealed(topic: Topic, kind: Kind, issued_at: u64) -> (Envelope, Packet) {
        let channel: Arc<str> = topic.channel(&Namespace::derive(&master())).into();
        let correlation = (kind == Kind::Request).then(Uuid::new_v4);
        let envelope = Envelope::new(
            topic,
            kind,
            Uuid::new_v4(),
            correlation,
            Bytes::new(),
            Bytes::from(serde_json::to_vec(&title()).unwrap()),
        );
        let bytes = envelope.encode(&channel, &Sealer::derive(&master()), issued_at);
        (envelope, Packet { channel, bytes })
    }

    fn ago(elapsed: Duration) -> u64 {
        clock::now() - milliseconds(elapsed)
    }

    #[tokio::test]
    async fn a_request_captured_longer_ago_than_its_lifetime_is_dropped() {
        let hub = Loopback::new();
        let receiver = Bus::attach(&hub, &master());
        let chat = Uuid::new_v4();
        let mut control = receiver.subscribe(Topic::ChatControl(chat)).await;
        let lifetime = Kind::Request.lifetime();
        let (_, replayed) = sealed(
            Topic::ChatControl(chat),
            Kind::Request,
            ago(lifetime + Duration::from_secs(1)),
        );
        let (recent, packet) = sealed(
            Topic::ChatControl(chat),
            Kind::Request,
            ago(lifetime - Duration::from_secs(2)),
        );

        hub.send(replayed);
        silent(&mut control).await;
        hub.send(packet);

        assert_eq!(next(&mut control).await, recent);
    }

    #[tokio::test]
    async fn a_message_is_dropped_once_older_than_its_own_lifetime() {
        let hub = Loopback::new();
        let receiver = Bus::attach(&hub, &master());
        let mut titles = receiver.subscribe(Topic::Titles).await;
        let (_, stale) = sealed(
            Topic::Titles,
            Kind::Message,
            ago(Kind::Message.lifetime() + Duration::from_secs(1)),
        );
        let (older_than_a_request, packet) = sealed(
            Topic::Titles,
            Kind::Message,
            ago(Kind::Request.lifetime() + Duration::from_secs(1)),
        );

        hub.send(stale);
        silent(&mut titles).await;
        hub.send(packet);

        assert_eq!(next(&mut titles).await, older_than_a_request);
    }

    #[tokio::test]
    async fn an_envelope_stamped_further_ahead_than_the_clock_skew_is_dropped() {
        let hub = Loopback::new();
        let receiver = Bus::attach(&hub, &master());
        let mut titles = receiver.subscribe(Topic::Titles).await;
        let (_, ahead) = sealed(
            Topic::Titles,
            Kind::Message,
            clock::now() + milliseconds(SKEW + Duration::from_secs(2)),
        );

        hub.send(ahead);

        silent(&mut titles).await;
    }

    #[tokio::test]
    async fn a_peer_receives_what_another_publishes_but_the_publisher_does_not() {
        let (first, second) = pair();
        let mut theirs = second.subscribe(Topic::Titles).await;
        let mut ours = first.subscribe(Topic::Titles).await;
        let sent = title();

        first.publish(Topic::Titles, &sent);

        let envelope = next(&mut theirs).await;
        assert_eq!(envelope.payload::<Title>().unwrap(), sent);
        assert_eq!(envelope.origin(), first.instance());
        assert_eq!(envelope.kind(), Kind::Message);
        assert_eq!(envelope.topic(), Topic::Titles);
        silent(&mut ours).await;
    }

    #[tokio::test]
    async fn raw_payloads_arrive_byte_for_byte_with_their_header() {
        let (first, second) = pair();
        let chat = Uuid::new_v4();
        let mut frames = second.subscribe(Topic::ChatFrames(chat)).await;
        let payload = Bytes::from_static(br#"{"type":"chunk","content":"Hel"}"#);
        let header = Frame {
            sequence: 3,
            terminator: false,
        };

        first.publish_raw(Topic::ChatFrames(chat), &header, payload.clone());

        let envelope = next(&mut frames).await;
        assert_eq!(envelope.bytes(), &payload);
        assert_eq!(envelope.header::<Frame>().unwrap(), header);
    }

    #[tokio::test]
    async fn a_subscriber_only_hears_its_own_topic() {
        let (first, second) = pair();
        let chat = Uuid::new_v4();
        let mut control = second.subscribe(Topic::ChatControl(chat)).await;

        first.publish(Topic::ChatFrames(chat), &title());
        first.publish(Topic::ChatControl(Uuid::new_v4()), &title());

        silent(&mut control).await;
    }

    #[tokio::test]
    async fn a_request_is_answered_by_a_peer() {
        let (asker, answerer) = pair();
        let answering_instance = answerer.instance();
        let chat = Uuid::new_v4();
        let mut control = answerer.subscribe(Topic::ChatControl(chat)).await;
        tokio::spawn(async move {
            let request = next(&mut control).await;
            assert_eq!(request.kind(), Kind::Request);
            let asked: Title = request.payload().unwrap();
            answerer.reply(
                &request,
                &Title {
                    chat: asked.chat,
                    title: format!("{} (acknowledged)", asked.title),
                },
            );
        });

        let reply = asker
            .request(Topic::ChatControl(chat), &title(), WAIT)
            .await
            .expect("the peer answers");

        assert_eq!(reply.kind(), Kind::Reply);
        assert_eq!(reply.origin(), answering_instance);
        assert_eq!(
            reply.payload::<Title>().unwrap().title,
            "Quarterly plan (acknowledged)"
        );
        assert!(asker.shared.pending.is_empty());
    }

    #[tokio::test]
    async fn a_request_nobody_answers_returns_none_after_its_timeout() {
        let (asker, _peer) = pair();
        let started = tokio::time::Instant::now();

        let reply = asker
            .request(Topic::ChatControl(Uuid::new_v4()), &title(), SILENCE)
            .await;

        assert!(reply.is_none());
        assert!(started.elapsed() >= SILENCE);
        assert!(asker.shared.pending.is_empty());
    }

    #[tokio::test]
    async fn replying_to_a_plain_message_sends_nothing() {
        let (first, second) = pair();
        let mut home = first.subscribe(Topic::Instance(first.instance())).await;
        let mut titles = second.subscribe(Topic::Titles).await;
        first.publish(Topic::Titles, &title());
        let message = next(&mut titles).await;

        second.reply(&message, &title());

        silent(&mut home).await;
    }

    #[tokio::test]
    async fn a_message_to_this_instance_is_delivered_without_a_peer() {
        let bus = Bus::loopback(&master());
        let mut home = bus.subscribe(Topic::Instance(bus.instance())).await;
        let sent = title();

        bus.publish(Topic::Instance(bus.instance()), &sent);

        assert_eq!(next(&mut home).await.payload::<Title>().unwrap(), sent);
        assert!(!bus.connected());
    }

    #[tokio::test]
    async fn a_bus_with_another_master_key_hears_nothing() {
        let hub = Loopback::new();
        let sender = Bus::attach(&hub, &master());
        let stranger = Bus::attach(&hub, &secret::derive(OTHER_PASSPHRASE).unwrap());
        let mut titles = stranger.subscribe(Topic::Titles).await;

        sender.publish(Topic::Titles, &title());

        silent(&mut titles).await;
    }

    #[tokio::test]
    async fn a_tampered_or_forged_envelope_is_dropped() {
        let hub = Loopback::new();
        let receiver = Bus::attach(&hub, &master());
        let mut titles = receiver.subscribe(Topic::Titles).await;
        let channel: Arc<str> = Topic::Titles.channel(&Namespace::derive(&master())).into();
        let envelope = Envelope::new(
            Topic::Titles,
            Kind::Message,
            Uuid::new_v4(),
            None,
            Bytes::new(),
            Bytes::from(serde_json::to_vec(&title()).unwrap()),
        );
        let genuine = envelope.encode(&channel, &Sealer::derive(&master()), clock::now());
        let mut tampered = genuine.to_vec();
        let inside_payload = tampered.len() - 40;
        tampered[inside_payload] ^= 0x01;
        let forged = envelope.encode(
            &channel,
            &Sealer::derive(&secret::derive(OTHER_PASSPHRASE).unwrap()),
            clock::now(),
        );

        for bytes in [Bytes::from(tampered), forged] {
            hub.send(Packet {
                channel: Arc::clone(&channel),
                bytes,
            });
        }
        silent(&mut titles).await;

        hub.send(Packet {
            channel,
            bytes: genuine,
        });
        assert_eq!(next(&mut titles).await, envelope);
    }

    #[tokio::test]
    async fn the_last_subscription_to_drop_unsubscribes() {
        let bus = Bus::loopback(&master());
        let first = bus.subscribe(Topic::Actions).await;
        let second = bus.subscribe(Topic::Actions).await;
        let channel = Topic::Actions.channel(&Namespace::derive(&master()));

        drop(first);
        assert_eq!(
            bus.shared
                .channels
                .get(channel.as_str())
                .unwrap()
                .subscribers,
            1
        );
        drop(second);
        assert!(bus.shared.channels.get(channel.as_str()).is_none());
    }

    #[tokio::test]
    async fn dropping_a_subscription_to_this_instance_keeps_replies_flowing() {
        let (asker, answerer) = pair();
        drop(asker.subscribe(Topic::Instance(asker.instance())).await);
        let chat = Uuid::new_v4();
        let mut control = answerer.subscribe(Topic::ChatControl(chat)).await;
        tokio::spawn(async move {
            let request = next(&mut control).await;
            answerer.reply(&request, &title());
        });

        let reply = asker
            .request(Topic::ChatControl(chat), &title(), WAIT)
            .await;

        assert!(reply.is_some());
    }

    #[tokio::test]
    async fn a_lone_loopback_reaches_no_peers_until_another_bus_attaches() {
        let hub = Loopback::new();
        let bus = Bus::attach(&hub, &master());
        assert!(!bus.shared.transport.reaches_peers());

        let _peer = Bus::attach(&hub, &master());

        assert!(bus.shared.transport.reaches_peers());
    }

    #[tokio::test]
    async fn a_url_the_bus_cannot_use_leaves_it_on_the_loopback_reporting_zero() {
        let reading = Arc::new(Reading::default());
        let bus = Bus::connect(
            "not a url",
            &master(),
            Gauge::from_arc(Arc::clone(&reading)),
        );
        let mut home = bus.subscribe(Topic::Instance(bus.instance())).await;

        bus.publish(Topic::Instance(bus.instance()), &title());

        next(&mut home).await;
        assert!(!bus.connected());
        assert!(reading.written());
        assert_eq!(reading.value(), 0.0);
    }
}
