use std::collections::HashSet;
use std::fs;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use abnegate_secret::MasterKey;
use async_trait::async_trait;
use axum::http::StatusCode;
use rustls::ServerConfig;
use rustls::crypto::aws_lc_rs;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::time::{Instant, sleep};
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};
use zone_enterprise::Tier;
use zone_enterprise::audit::destination::{Secrets, Settings};
use zone_enterprise::audit::signature::{SIGNATURE_HEADER, TIMESTAMP_HEADER, verify};
use zone_enterprise::audit::{Event, FailureCode, Format, object_key};
use zone_server::db::audit::deliveries::{self, Delivery};
use zone_server::db::audit::destinations::DestinationRecord;
use zone_server::db::audit::position::Position;
use zone_server::db::audit::scope::Scope;
use zone_server::db::audit::{actions, resources};
use zone_server::services::audit::delivery::{Cadence, Open};
use zone_server::services::audit::destinations;
use zone_server::services::audit::retention;
use zone_server::services::audit::transport::s3::S3;
use zone_server::services::audit::transport::syslog::Syslog;
use zone_server::services::audit::transport::webhook::Webhook;
use zone_server::services::audit::transport::{Failure, Transport, client};
use zone_server::services::hosts::Hosts;
use zone_server::state::AppState;
use zone_server::workers::audit::drain;

use crate::common::audit::{header, settle};
use crate::common::policy::Fixture;
use crate::common::sso::{active, lapsed};
use crate::common::{TestClient, TestResponse};

const PREFIX: &str = "audit_streaming";
const CADENCE: Cadence = Cadence {
    tick: Duration::from_millis(50),
    lease: Duration::from_secs(30),
    concurrency: 4,
};
const SERVER: &str = "/api/audit-destinations";
const SIGNING_SECRET: &str = "whsec-streaming-0123456789abcdef";
const ACCESS_KEY_ID: &str = "AKIDSTREAMINGEXAMPLE";
const SECRET_ACCESS_KEY: &str = "streaming/secret+access/key";
const BUCKET: &str = "zone-audit";
const REGION: &str = "eu-west-1";
const OBJECT_PREFIX: &str = "zone/audit/";
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audit");
const LOOPBACK: &str = "127.0.0.1";
const HOSTNAME: &str = "zone-streaming";
const BOM: char = '\u{FEFF}';
const WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(20);
const STALL: Duration = Duration::from_secs(60);
const OUTAGE: i32 = 3;
const RESTORED: i64 = 1 << 40;
const MISMATCHED: &str = "The destination's settings and secrets are of different kinds";

struct Transports {
    client: reqwest::Client,
    key: Arc<MasterKey>,
    hosts: Hosts,
}

impl Transports {
    fn of(state: &AppState) -> Self {
        Self {
            client: client::client(),
            key: state.master_key().clone(),
            hosts: state.config().endpoint_hosts.clone(),
        }
    }
}

#[async_trait]
impl Open for Transports {
    async fn open(&self, destination: &DestinationRecord) -> Result<Box<dyn Transport>, Failure> {
        match destinations::open(destination, &self.key, &self.hosts)? {
            (Settings::Webhook(settings), Secrets::Webhook(secrets)) => Ok(Box::new(Webhook::new(
                &settings,
                &secrets,
                self.client.clone(),
            ))),
            (Settings::S3(settings), Secrets::S3(secrets)) => {
                Ok(Box::new(S3::new(&settings, &secrets, self.client.clone())))
            }
            (Settings::Syslog(settings), Secrets::Syslog(secrets)) => {
                Ok(Box::new(Syslog::new(&settings, &secrets)?))
            }
            _ => Err(Failure::new(FailureCode::Configuration, MISMATCHED)),
        }
    }
}

#[derive(Deserialize)]
struct Payload {
    events: Vec<Event>,
}

#[derive(Default)]
struct Hook {
    down: AtomicBool,
    stalled: AtomicBool,
    arrived: Notify,
    forged: AtomicUsize,
    accepted: Mutex<Vec<Vec<Event>>>,
    refused: Mutex<Vec<Vec<Event>>>,
}

impl Hook {
    fn accepted(&self) -> Vec<Vec<Uuid>> {
        self.accepted
            .lock()
            .unwrap()
            .iter()
            .map(|events| events.iter().map(|event| event.id).collect())
            .collect()
    }

    fn refused(&self) -> Vec<Vec<Uuid>> {
        self.refused
            .lock()
            .unwrap()
            .iter()
            .map(|events| events.iter().map(|event| event.id).collect())
            .collect()
    }

    fn received(&self) -> Vec<Uuid> {
        self.accepted().concat()
    }

    fn organizations(&self) -> HashSet<Option<Uuid>> {
        self.accepted
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .map(|event| event.organization_id)
            .collect()
    }
}

struct Answer(Arc<Hook>);

impl Respond for Answer {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let hook = &self.0;
        let timestamp = header(request, TIMESTAMP_HEADER)
            .parse()
            .unwrap_or_default();
        let signature = header(request, SIGNATURE_HEADER);
        if !verify(
            SIGNING_SECRET.as_bytes(),
            timestamp,
            &request.body,
            signature,
        ) {
            hook.forged.fetch_add(1, Ordering::SeqCst);
            return ResponseTemplate::new(StatusCode::UNAUTHORIZED.as_u16());
        }
        let events = serde_json::from_slice::<Payload>(&request.body)
            .expect("the webhook body carries events")
            .events;
        if hook.down.load(Ordering::SeqCst) {
            hook.refused.lock().unwrap().push(events);
            return ResponseTemplate::new(StatusCode::SERVICE_UNAVAILABLE.as_u16());
        }
        hook.accepted.lock().unwrap().push(events);
        if hook.stalled.load(Ordering::SeqCst) {
            hook.arrived.notify_one();
            return ResponseTemplate::new(StatusCode::OK.as_u16()).set_delay(STALL);
        }
        ResponseTemplate::new(StatusCode::OK.as_u16())
    }
}

struct Receiver {
    server: MockServer,
    hook: Arc<Hook>,
}

impl Receiver {
    async fn start() -> Self {
        let server = MockServer::start().await;
        let hook = Arc::new(Hook::default());
        Mock::given(method("POST"))
            .respond_with(Answer(hook.clone()))
            .mount(&server)
            .await;
        Self { server, hook }
    }

    fn body(&self, name: &str) -> Value {
        json!({
            "name": name,
            "settings": {"kind": "webhook", "url": format!("{}/hook", self.server.uri())},
            "secrets": {"signing_secret": SIGNING_SECRET},
        })
    }
}

#[derive(Clone)]
struct Object {
    path: String,
    body: Vec<u8>,
    content_type: String,
    authorization: String,
}

#[derive(Default)]
struct Bucket {
    refusing: AtomicBool,
    stored: Mutex<Vec<Object>>,
    refused: Mutex<Vec<Object>>,
}

struct Store(Arc<Bucket>);

impl Respond for Store {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let bucket = &self.0;
        let object = Object {
            path: request.url.path().to_owned(),
            body: request.body.clone(),
            content_type: header(request, "content-type").to_owned(),
            authorization: header(request, "authorization").to_owned(),
        };
        if bucket.refusing.swap(false, Ordering::SeqCst) {
            bucket.refused.lock().unwrap().push(object);
            return ResponseTemplate::new(StatusCode::SERVICE_UNAVAILABLE.as_u16());
        }
        bucket.stored.lock().unwrap().push(object);
        ResponseTemplate::new(StatusCode::OK.as_u16())
    }
}

#[derive(Default)]
struct Collector {
    messages: Mutex<Vec<String>>,
}

impl Collector {
    fn count(&self) -> usize {
        self.messages.lock().unwrap().len()
    }
}

struct Stream {
    fixture: Fixture,
    open: Transports,
    owner: Uuid,
}

impl Stream {
    async fn new() -> Self {
        let fixture = Fixture::new(PREFIX).await;
        fixture.license(Tier::Enterprise).await;
        let open = Transports::of(fixture.state());
        Self {
            fixture,
            open,
            owner: Uuid::new_v4(),
        }
    }

    fn pool(&self) -> &PgPool {
        self.fixture.pool()
    }

    fn client(&self) -> &TestClient {
        &self.fixture.sso.client
    }

    fn token(&self) -> &str {
        &self.fixture.owner.token
    }

    fn organization(&self) -> Scope {
        Scope::Organization(self.fixture.organization)
    }

    async fn create(&self, scope: Scope, body: &Value) -> Uuid {
        let response = self
            .client()
            .post_json_auth(&destinations(scope), body, self.token())
            .await;
        response.assert_status(StatusCode::CREATED);
        id(&response)
    }

    async fn view(&self, scope: Scope, destination: Uuid) -> Value {
        let response = self
            .client()
            .get_auth(
                &format!("{}/{destination}", destinations(scope)),
                self.token(),
            )
            .await;
        response.assert_status(StatusCode::OK);
        response.json_value()
    }

    async fn retention(&self) -> Value {
        let response = self
            .client()
            .get_auth(&retention_path(self.fixture.organization), self.token())
            .await;
        response.assert_status(StatusCode::OK);
        response.json_value()
    }

    async fn written(&self, scope: Scope, count: usize) -> Vec<Uuid> {
        sqlx::query_scalar(
            "WITH written AS ( \
                 INSERT INTO audit_logs (organization_id, action, resource_type) \
                 SELECT $1, $2, $3 FROM generate_series(1, $4) \
                 RETURNING id, sequence) \
             SELECT id FROM written ORDER BY sequence",
        )
        .bind(scope.organization_id())
        .bind(actions::SETTINGS_UPDATED)
        .bind(resources::ORGANIZATION)
        .bind(i32::try_from(count).unwrap())
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    async fn backdate(&self, ids: &[Uuid], by: &str) {
        sqlx::query(
            "UPDATE audit_logs SET created_at = created_at - $2::interval WHERE id = ANY($1)",
        )
        .bind(ids)
        .bind(by)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn since(&self, scope: Scope, after: Position) -> Vec<Uuid> {
        sqlx::query_scalar(
            "SELECT id FROM audit_logs \
             WHERE organization_id IS NOT DISTINCT FROM $1 \
               AND (transaction_id, sequence) > ($2, $3) \
             ORDER BY transaction_id, sequence",
        )
        .bind(scope.organization_id())
        .bind(after.transaction_id)
        .bind(after.sequence)
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    async fn head(&self, scope: Scope) -> Position {
        let (transaction_id, sequence): (i64, i64) = sqlx::query_as(
            "SELECT transaction_id, sequence FROM audit_logs \
             WHERE organization_id IS NOT DISTINCT FROM $1 \
             ORDER BY transaction_id DESC, sequence DESC LIMIT 1",
        )
        .bind(scope.organization_id())
        .fetch_one(self.pool())
        .await
        .unwrap();
        Position {
            transaction_id,
            sequence,
        }
    }

    async fn surviving(&self, ids: &[Uuid]) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE id = ANY($1)")
            .bind(ids)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn delivery(&self, destination: Uuid) -> Delivery {
        deliveries::get(self.pool(), destination)
            .await
            .unwrap()
            .expect("the delivery exists")
    }

    async fn due(&self, destination: Uuid) {
        sqlx::query(
            "UPDATE audit_deliveries SET next_attempt_at = now() - interval '1 second' \
             WHERE destination_id = $1",
        )
        .bind(destination)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn expire(&self, destination: Uuid) {
        sqlx::query(
            "UPDATE audit_deliveries SET lease_expires_at = now() - interval '1 second' \
             WHERE destination_id = $1",
        )
        .bind(destination)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn drain(&self) -> usize {
        self.drain_as(self.owner).await
    }

    async fn drain_as(&self, owner: Uuid) -> usize {
        drain(self.fixture.state(), owner, CADENCE, &self.open)
            .await
            .expect("the tick drains")
    }

    async fn cleanup(self) {
        self.fixture.sso.cleanup().await;
    }
}

fn destinations(scope: Scope) -> String {
    match scope {
        Scope::Organization(organization) => {
            format!("/api/organizations/{organization}/audit-destinations")
        }
        Scope::Server => SERVER.to_owned(),
    }
}

fn retention_path(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/audit-retention")
}

fn id(response: &TestResponse) -> Uuid {
    response.json_value()["id"]
        .as_str()
        .expect("the destination has an id")
        .parse()
        .unwrap()
}

fn unique(ids: &[Uuid]) -> Vec<Uuid> {
    let mut seen = HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

fn fixture(name: &str) -> String {
    fs::read_to_string(format!("{FIXTURES}/{name}")).expect("the audit TLS fixture exists")
}

fn certificates(name: &str) -> Vec<CertificateDer<'static>> {
    CertificateDer::pem_slice_iter(fixture(name).as_bytes())
        .collect::<Result<_, _>>()
        .expect("the fixture holds PEM certificates")
}

fn frames(stream: &[u8]) -> Vec<String> {
    let mut frames = Vec::new();
    let mut rest = stream;
    while !rest.is_empty() {
        let space = rest
            .iter()
            .position(|byte| *byte == b' ')
            .expect("every frame starts with its octet count");
        let length: usize = std::str::from_utf8(&rest[..space])
            .unwrap()
            .parse()
            .expect("the octet count is a number");
        let end = space + 1 + length;
        frames.push(String::from_utf8(rest[space + 1..end].to_vec()).expect("frames are UTF-8"));
        rest = &rest[end..];
    }
    frames
}

fn parsed(message: &str) -> Event {
    let (_, body) = message
        .split_once(BOM)
        .expect("the message body follows a BOM");
    serde_json::from_str(body).expect("the message body is an event")
}

fn ndjson(body: &[u8]) -> Vec<Event> {
    serde_json::Deserializer::from_slice(body)
        .into_iter()
        .collect::<Result<_, _>>()
        .expect("the object is NDJSON events")
}

async fn syslog_receiver() -> (u16, Arc<Collector>) {
    let provider = Arc::new(aws_lc_rs::default_provider());
    let key = PrivateKeyDer::from_pem_slice(fixture("server.key").as_bytes())
        .expect("the server key is PEM");
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("the provider supports the default versions")
        .with_no_client_auth()
        .with_single_cert(certificates("server.pem"), key)
        .expect("the server certificate matches its key");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind((LOOPBACK, 0))
        .await
        .expect("a loopback port");
    let port = listener.local_addr().unwrap().port();
    let collector = Arc::new(Collector::default());
    let collecting = collector.clone();
    tokio::spawn(async move {
        loop {
            let Ok((connection, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            let collecting = collecting.clone();
            tokio::spawn(async move {
                let mut stream = acceptor.accept(connection).await?;
                let mut bytes = Vec::new();
                stream.read_to_end(&mut bytes).await?;
                collecting.messages.lock().unwrap().extend(frames(&bytes));
                Ok::<(), io::Error>(())
            });
        }
    });
    (port, collector)
}

async fn collected(collector: &Collector, count: usize) {
    let deadline = Instant::now() + WAIT;
    while collector.count() < count {
        assert!(
            Instant::now() < deadline,
            "the syslog receiver got {} of {count} messages",
            collector.count()
        );
        sleep(POLL).await;
    }
}

#[tokio::test]
async fn a_webhook_that_is_down_for_several_ticks_ends_with_every_event_once() {
    let stream = Stream::new().await;
    let scope = stream.organization();
    let receiver = Receiver::start().await;
    receiver.hook.down.store(true, Ordering::SeqCst);
    let destination = stream.create(scope, &receiver.body("SIEM")).await;
    let start = stream.delivery(destination).await.delivered;
    stream.written(scope, 250).await;
    settle(stream.pool()).await;

    for attempt in 1..=OUTAGE {
        assert_eq!(stream.drain().await, 0, "the receiver is down");
        let view = stream.view(scope, destination).await;
        assert_eq!(view["delivery"]["health"], "failing");
        assert_eq!(view["delivery"]["attempts"], attempt);
        assert_eq!(view["delivery"]["last_error_code"], "status");
        assert_eq!(view["delivery"]["last_error"], "HTTP 503");
        assert!(view["delivery"]["backlog"].as_i64().unwrap() >= 250);
        stream.due(destination).await;
    }
    receiver.hook.down.store(false, Ordering::SeqCst);
    stream.drain().await;
    settle(stream.pool()).await;
    stream.due(destination).await;
    stream.drain().await;

    let expected = stream.since(scope, start).await;
    let received = receiver.hook.received();
    assert_eq!(received, expected, "every event arrives once, in order");
    let first = receiver.hook.accepted()[0].clone();
    assert_eq!(first.len(), 100, "webhook batches carry 100 events");
    assert_eq!(
        receiver.hook.refused(),
        vec![first; usize::try_from(OUTAGE).unwrap()],
        "every refused tick resent the identical pinned batch"
    );
    assert_eq!(receiver.hook.forged.load(Ordering::SeqCst), 0);
    assert_eq!(
        stream.delivery(destination).await.delivered,
        stream.head(scope).await,
        "the cursor ends at the head"
    );
    let view = stream.view(scope, destination).await;
    assert_eq!(view["delivery"]["health"], "healthy");
    assert_eq!(view["delivery"]["backlog"], 0);
    assert_eq!(view["delivery"]["attempts"], 0);
    assert_eq!(view["delivery"]["delivered_count"], expected.len());
    assert!(view["delivery"]["last_success_at"].is_string());
    stream.cleanup().await;
}

#[tokio::test]
async fn a_restarted_owner_resends_a_stranded_batch_and_loses_nothing() {
    let stream = Stream::new().await;
    let scope = stream.organization();
    let receiver = Receiver::start().await;
    let destination = stream.create(scope, &receiver.body("SIEM")).await;
    let start = stream.delivery(destination).await.delivered;
    stream.written(scope, 150).await;
    settle(stream.pool()).await;
    let crashed = Uuid::new_v4();

    receiver.hook.stalled.store(true, Ordering::SeqCst);
    {
        let crash = stream.drain_as(crashed);
        tokio::pin!(crash);
        tokio::select! {
            _ = &mut crash => panic!("a stalled receiver never answers the crashed owner"),
            () = receiver.hook.arrived.notified() => {}
        }
    }
    receiver.hook.stalled.store(false, Ordering::SeqCst);
    let stranded = stream.delivery(destination).await;
    assert_eq!(stranded.lease_owner, Some(crashed));
    let pinned = stranded.batch.expect("the crashed owner pinned its batch");

    assert_eq!(
        stream.drain().await,
        0,
        "a live lease keeps the restarted owner away"
    );
    stream.expire(destination).await;
    stream.drain().await;
    settle(stream.pool()).await;
    stream.due(destination).await;
    stream.drain().await;

    let accepted = receiver.hook.accepted();
    assert_eq!(
        accepted[0], accepted[1],
        "the restarted owner resends the stranded batch identically"
    );
    let expected = stream.since(scope, start).await;
    assert_eq!(
        unique(&receiver.hook.received()),
        expected,
        "deduplicated by id, every event arrived in order"
    );
    assert!(receiver.hook.received().len() > expected.len());
    let delivery = stream.delivery(destination).await;
    let head = stream.head(scope).await;
    assert_eq!(delivery.delivered, head);
    assert!(delivery.delivered >= pinned);
    assert!(
        !deliveries::advance(stream.pool(), destination, crashed, head, 0)
            .await
            .unwrap(),
        "the crashed owner can no longer advance"
    );
    stream.cleanup().await;
}

#[tokio::test]
async fn s3_receives_deterministic_ndjson_objects_across_a_retry() {
    let stream = Stream::new().await;
    let scope = stream.organization();
    let server = MockServer::start().await;
    let bucket = Arc::new(Bucket::default());
    bucket.refusing.store(true, Ordering::SeqCst);
    Mock::given(method("PUT"))
        .respond_with(Store(bucket.clone()))
        .mount(&server)
        .await;
    let destination = stream
        .create(
            scope,
            &json!({
                "name": "Archive",
                "settings": {
                    "kind": "s3", "bucket": BUCKET, "region": REGION,
                    "endpoint": server.uri(), "prefix": OBJECT_PREFIX, "addressing": "path"
                },
                "secrets": {
                    "access_key_id": ACCESS_KEY_ID, "secret_access_key": SECRET_ACCESS_KEY
                },
            }),
        )
        .await;
    let start = stream.delivery(destination).await.delivered;
    stream.written(scope, 1_500).await;
    let aged = stream.since(scope, start).await;
    stream.backdate(&aged, "2 minutes").await;
    settle(stream.pool()).await;

    assert_eq!(
        stream.drain().await,
        0,
        "the store refuses the first object"
    );
    assert_eq!(
        stream.view(scope, destination).await["delivery"]["last_error_code"],
        "status"
    );
    stream.due(destination).await;
    stream.drain().await;

    let refused = bucket.refused.lock().unwrap().clone();
    let stored = bucket.stored.lock().unwrap().clone();
    assert_eq!(refused.len(), 1);
    assert_eq!(
        (&refused[0].path, &refused[0].body),
        (&stored[0].path, &stored[0].body),
        "the retry PUTs the identical object at the identical key"
    );
    let expected = stream.since(scope, start).await;
    let mut delivered = Vec::new();
    for object in &stored {
        let events = ndjson(&object.body);
        let first = &events[0];
        let key = object_key(
            OBJECT_PREFIX,
            scope.organization_id(),
            first.created_at,
            first.id,
        );
        assert_eq!(object.path, format!("/{BUCKET}/{key}"));
        assert_eq!(object.content_type, Format::Ndjson.content_type());
        assert_eq!(object.body.last(), Some(&b'\n'));
        assert!(
            object
                .authorization
                .contains(&format!("Credential={ACCESS_KEY_ID}/")),
            "{}",
            object.authorization
        );
        assert!(!object.authorization.contains(SECRET_ACCESS_KEY));
        assert!(
            events
                .iter()
                .all(|event| event.organization_id == scope.organization_id())
        );
        delivered.extend(events.iter().map(|event| event.id));
    }
    assert_eq!(
        stored
            .iter()
            .map(|object| ndjson(&object.body).len())
            .collect::<Vec<_>>(),
        vec![1_000, expected.len() - 1_000],
        "a full object, then the aged remainder"
    );
    assert_eq!(
        delivered, expected,
        "every event lands in exactly one object"
    );
    assert_eq!(
        stream.delivery(destination).await.delivered,
        stream.head(scope).await
    );
    stream.cleanup().await;
}

#[tokio::test]
async fn a_tls_syslog_receiver_gets_a_frame_for_every_event() {
    let stream = Stream::new().await;
    let scope = stream.organization();
    let (port, collector) = syslog_receiver().await;
    let destination = stream
        .create(
            scope,
            &json!({
                "name": "Syslog",
                "settings": {
                    "kind": "syslog", "host": LOOPBACK, "port": port,
                    "protocol": "tls", "hostname": HOSTNAME
                },
                "secrets": {"ca_certificate": fixture("ca.pem")},
            }),
        )
        .await;
    let start = stream.delivery(destination).await.delivered;
    stream.written(scope, 700).await;
    settle(stream.pool()).await;

    stream.drain().await;
    let delivery = stream.delivery(destination).await;
    assert_eq!(delivery.last_error, None, "the receiver accepts every pass");
    let expected = stream.since(scope, start).await;
    collected(&collector, expected.len()).await;

    let messages = collector.messages.lock().unwrap().clone();
    let mut received: Vec<Event> = messages.iter().map(|message| parsed(message)).collect();
    received.sort_by_key(|event| expected.iter().position(|id| *id == event.id));
    assert_eq!(
        received.iter().map(|event| event.id).collect::<Vec<_>>(),
        expected,
        "every event arrives in exactly one frame"
    );
    assert!(
        messages
            .iter()
            .all(|message| message.split(' ').nth(2) == Some(HOSTNAME)),
        "every frame carries the configured hostname"
    );
    assert_eq!(
        stream.delivery(destination).await.delivered,
        stream.head(scope).await
    );
    let view = stream.view(scope, destination).await;
    assert_eq!(view["delivery"]["health"], "healthy");
    assert_eq!(view["delivery"]["delivered_count"], expected.len());
    stream.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_pauses_streaming_while_the_backlog_grows_until_renewal() {
    let stream = Stream::new().await;
    let scope = stream.organization();
    let receiver = Receiver::start().await;
    let destination = stream.create(scope, &receiver.body("SIEM")).await;
    let start = stream.delivery(destination).await.delivered;
    stream.written(scope, 3).await;
    settle(stream.pool()).await;
    stream.drain().await;
    let streamed = receiver.hook.received().len();
    assert_eq!(streamed, stream.since(scope, start).await.len());

    stream
        .fixture
        .store_license(&lapsed(Tier::Enterprise))
        .await;
    let mut backlog = 0;
    for count in [4, 5] {
        stream.written(scope, count).await;
        settle(stream.pool()).await;
        stream.due(destination).await;
        assert_eq!(stream.drain().await, 0, "a lapsed license streams nothing");
        backlog += count;
        let view = stream.view(scope, destination).await;
        assert_eq!(view["delivery"]["health"], "paused");
        assert_eq!(view["delivery"]["backlog"], backlog, "the backlog grows");
        assert_eq!(view["delivery"]["next_attempt_at"], Value::Null);
    }
    assert_eq!(receiver.hook.received().len(), streamed);

    stream.fixture.install(&active(Tier::Enterprise)).await;
    stream.due(destination).await;
    assert_eq!(stream.drain().await, backlog, "renewal drains the backlog");
    assert_eq!(
        receiver.hook.received(),
        stream.since(scope, start).await,
        "nothing is lost across the lapse"
    );
    let view = stream.view(scope, destination).await;
    assert_eq!(view["delivery"]["health"], "healthy");
    assert_eq!(view["delivery"]["backlog"], 0);
    stream.cleanup().await;
}

#[tokio::test]
async fn retention_holds_rows_for_a_failing_destination_and_deletes_them_once_it_recovers() {
    let stream = Stream::new().await;
    let scope = stream.organization();
    let receiver = Receiver::start().await;
    receiver.hook.down.store(true, Ordering::SeqCst);
    let destination = stream.create(scope, &receiver.body("SIEM")).await;
    let expired = stream.written(scope, 20).await;
    stream.backdate(&expired, "10 days").await;
    stream
        .client()
        .put_json_auth(
            &retention_path(stream.fixture.organization),
            &json!({"days": 1}),
            stream.token(),
        )
        .await
        .assert_status(StatusCode::OK);
    settle(stream.pool()).await;
    assert_eq!(stream.drain().await, 0, "the destination is failing");

    assert_eq!(retention::sweep(stream.fixture.state()).await.unwrap(), 0);
    assert_eq!(
        stream.surviving(&expired).await,
        20,
        "undelivered rows are held"
    );
    let status = stream.retention().await;
    assert_eq!(status["expired"], 20);
    assert_eq!(status["held"], 20);
    assert_eq!(
        status["holders"],
        json!([{"destination_id": destination, "name": "SIEM", "wait": "failing"}])
    );

    receiver.hook.down.store(false, Ordering::SeqCst);
    stream.due(destination).await;
    stream.drain().await;
    settle(stream.pool()).await;
    assert_eq!(retention::sweep(stream.fixture.state()).await.unwrap(), 20);
    assert_eq!(
        stream.surviving(&expired).await,
        0,
        "delivered rows are deleted"
    );
    assert!(
        expired
            .iter()
            .all(|id| receiver.hook.received().contains(id)),
        "every deleted row was delivered first"
    );
    let status = stream.retention().await;
    assert_eq!(status["expired"], 0);
    assert_eq!(status["held"], 0);
    assert_eq!(status["holders"], json!([]));
    assert_eq!(status["deleted_count"], 20);
    assert!(status["swept_at"].is_string());
    stream.cleanup().await;
}

#[tokio::test]
async fn organization_and_server_destinations_never_receive_each_others_events() {
    let stream = Stream::new().await;
    let first = stream.organization();
    let (other, _) = stream
        .fixture
        .another_organization(&stream.fixture.owner)
        .await;
    let second = Scope::Organization(other);
    let scopes = [first, second, Scope::Server];
    let mut streams = Vec::new();
    for scope in scopes {
        let receiver = Receiver::start().await;
        let destination = stream.create(scope, &receiver.body("SIEM")).await;
        let start = stream.delivery(destination).await.delivered;
        streams.push((scope, receiver, destination, start));
    }
    for (scope, count) in scopes.into_iter().zip([3, 4, 5]) {
        stream.written(scope, count).await;
    }
    settle(stream.pool()).await;
    stream.drain().await;

    for (scope, receiver, destination, start) in &streams {
        let delivery = stream.delivery(*destination).await;
        assert_eq!(
            delivery.last_error, None,
            "{scope:?} delivers without failing"
        );
        assert_eq!(
            receiver.hook.received(),
            stream.since(*scope, *start).await,
            "{scope:?} receives exactly its own events"
        );
        assert_eq!(
            receiver.hook.organizations(),
            HashSet::from([scope.organization_id()]),
            "{scope:?} receives no other scope's events"
        );
        assert_eq!(delivery.delivered, stream.head(*scope).await);
    }
    stream.cleanup().await;
}

#[tokio::test]
async fn a_simulated_restore_before_a_tick_loses_nothing() {
    let stream = Stream::new().await;
    let scope = stream.organization();
    let receiver = Receiver::start().await;
    let destination = stream.create(scope, &receiver.body("SIEM")).await;
    let start = stream.delivery(destination).await.delivered;
    stream.written(scope, 3).await;
    settle(stream.pool()).await;
    stream.drain().await;
    stream.written(scope, 2).await;
    settle(stream.pool()).await;
    let before = stream.since(scope, start).await;

    sqlx::query("UPDATE audit_logs SET transaction_id = transaction_id + $1")
        .bind(RESTORED)
        .execute(stream.pool())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE audit_deliveries SET transaction_id = transaction_id + $2 \
         WHERE destination_id = $1",
    )
    .bind(destination)
    .bind(RESTORED)
    .execute(stream.pool())
    .await
    .unwrap();
    let after = stream.written(scope, 1).await;

    stream.drain().await;
    settle(stream.pool()).await;
    stream.due(destination).await;
    stream.drain().await;

    assert_eq!(
        unique(&receiver.hook.received()),
        [before, after].concat(),
        "every row written before and after the restore arrives, in order"
    );
    assert_eq!(
        stream.delivery(destination).await.delivered,
        stream.head(scope).await
    );
    stream.cleanup().await;
}
