use std::sync::{Arc, Weak};
use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use redis::aio::{ConnectionManager, ConnectionManagerConfig, PubSubSink, PubSubStream};
use redis::{Client, IntoConnectionInfo, RedisResult};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::command::Command;
use super::packet::Packet;
use super::shared::Shared;
use super::status::Status;
use super::throttle::Throttle;

const LIBRARY: &str = "zone-bus";
const BACKLOG: usize = 4096;
const BATCH: usize = 256;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
const HEARTBEAT: Duration = Duration::from_secs(10);
const FIRST_RETRY: Duration = Duration::from_millis(200);
const LAST_RETRY: Duration = Duration::from_secs(5);
const PUBLISH_RETRIES: usize = 1;
pub const LOG_INTERVAL: Duration = Duration::from_secs(60);

/// Valkey pub/sub as the bus's transport: one multiplexed connection that
/// publishes in pipelined batches, and one pub/sub connection that a task
/// keeps subscribed, reconnecting and resubscribing whenever it drops.
#[derive(Debug)]
pub struct Valkey {
    manager: ConnectionManager,
    publisher: mpsc::Sender<Packet>,
    commands: mpsc::UnboundedSender<Command>,
    dropped: Throttle,
}

impl Valkey {
    /// The client for `url`, named after the instance so `CLIENT LIST` shows
    /// which replica holds each connection, and the publishing connection,
    /// which connects on first use.
    pub fn client(url: &str, instance: Uuid) -> RedisResult<(Client, ConnectionManager)> {
        let info = url.into_connection_info()?;
        let settings = info
            .redis_settings()
            .clone()
            .set_lib_name(LIBRARY, instance.to_string());
        let client = Client::open(info.set_redis_settings(settings))?;
        let manager = client.get_connection_manager_lazy(
            ConnectionManagerConfig::new()
                .set_connection_timeout(Some(CONNECT_TIMEOUT))
                .set_response_timeout(Some(RESPONSE_TIMEOUT))
                .set_number_of_retries(PUBLISH_RETRIES),
        )?;
        Ok((client, manager))
    }

    pub fn start(
        client: Client,
        manager: ConnectionManager,
        shared: Weak<Shared>,
        status: Arc<Status>,
        stop: CancellationToken,
    ) -> Self {
        let (publisher, packets) = mpsc::channel(BACKLOG);
        let (commands, receiver) = mpsc::unbounded_channel();
        tokio::spawn(publish(manager.clone(), packets));
        tokio::spawn(subscribe(client, shared, receiver, status, stop));
        Self {
            manager,
            publisher,
            commands,
            dropped: Throttle::new(LOG_INTERVAL),
        }
    }

    pub fn send(&self, packet: Packet) {
        if let Err(TrySendError::Full(_)) = self.publisher.try_send(packet)
            && self.dropped.ready()
        {
            tracing::warn!(
                "The cluster bus fell {BACKLOG} envelopes behind Valkey and dropped new ones"
            );
        }
    }

    pub fn command(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// The multiplexed connection the bus publishes on, for commands that
    /// are not pub/sub.
    pub fn manager(&self) -> &ConnectionManager {
        &self.manager
    }
}

async fn publish(mut manager: ConnectionManager, mut packets: mpsc::Receiver<Packet>) {
    let failed = Throttle::new(LOG_INTERVAL);
    let mut batch = Vec::with_capacity(BATCH);
    while packets.recv_many(&mut batch, BATCH).await > 0 {
        let mut pipeline = redis::pipe();
        for packet in batch.drain(..) {
            pipeline
                .cmd("PUBLISH")
                .arg(&*packet.channel)
                .arg(packet.bytes.as_ref())
                .ignore();
        }
        if let Err(error) = pipeline.query_async::<()>(&mut manager).await
            && failed.ready()
        {
            tracing::warn!(%error, "Could not publish to the cluster bus; other replicas missed envelopes");
        }
    }
}

enum Ended {
    Stopped,
    Lost(String),
}

async fn subscribe(
    client: Client,
    shared: Weak<Shared>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    status: Arc<Status>,
    stop: CancellationToken,
) {
    let disconnected = Throttle::new(LOG_INTERVAL);
    let mut retry = FIRST_RETRY;
    loop {
        let attempt = tokio::select! {
            () = stop.cancelled() => break,
            attempt = connect(&client, &shared) => attempt,
        };
        let reason = match attempt {
            Ok(Some((mut sink, mut stream))) => {
                status.set(true);
                retry = FIRST_RETRY;
                tracing::info!("The cluster bus is subscribed on Valkey");
                let ended = serve(&mut sink, &mut stream, &shared, &mut commands, &stop).await;
                status.set(false);
                match ended {
                    Ended::Stopped => break,
                    Ended::Lost(reason) => reason,
                }
            }
            Ok(None) => break,
            Err(reason) => reason,
        };
        if disconnected.ready() {
            tracing::error!(
                %reason,
                "The cluster bus is not connected to Valkey (REDIS_URL); replicas cannot relay chats, answers or titles to each other until it reconnects"
            );
        }
        if !wait(retry, &mut commands, &stop).await {
            break;
        }
        retry = (retry * 2).min(LAST_RETRY);
    }
    status.set(false);
}

async fn connect(
    client: &Client,
    shared: &Weak<Shared>,
) -> Result<Option<(PubSubSink, PubSubStream)>, String> {
    let pubsub = tokio::time::timeout(CONNECT_TIMEOUT, client.get_async_pubsub())
        .await
        .map_err(|_| "connecting timed out".to_string())?
        .map_err(|error| error.to_string())?;
    let (mut sink, stream) = pubsub.split();
    let Some(channels) = shared.upgrade().map(|shared| shared.channels()) else {
        return Ok(None);
    };
    respond(sink.subscribe(&channels)).await?;
    Ok(Some((sink, stream)))
}

async fn serve(
    sink: &mut PubSubSink,
    stream: &mut PubSubStream,
    shared: &Weak<Shared>,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    stop: &CancellationToken,
) -> Ended {
    let mut heartbeat =
        tokio::time::interval_at(tokio::time::Instant::now() + HEARTBEAT, HEARTBEAT);
    loop {
        tokio::select! {
            () = stop.cancelled() => return Ended::Stopped,
            message = stream.next() => {
                let Some(message) = message else {
                    return Ended::Lost("the pub/sub connection closed".to_string());
                };
                let Some(shared) = shared.upgrade() else {
                    return Ended::Stopped;
                };
                shared.receive(
                    message.get_channel_name(),
                    Bytes::copy_from_slice(message.get_payload_bytes()),
                );
            }
            command = commands.recv() => {
                let outcome = match command {
                    None => return Ended::Stopped,
                    Some(Command::Subscribe { channel, confirmed }) => {
                        let outcome = respond(sink.subscribe(&*channel)).await;
                        confirmed.send_replace(true);
                        outcome
                    }
                    Some(Command::Unsubscribe { channel }) => {
                        respond(sink.unsubscribe(&*channel)).await
                    }
                };
                if let Err(reason) = outcome {
                    return Ended::Lost(reason);
                }
            }
            _ = heartbeat.tick() => {
                if let Err(reason) = respond(sink.ping::<redis::Value>()).await {
                    return Ended::Lost(format!("the heartbeat went unanswered: {reason}"));
                }
            }
        }
    }
}

async fn respond<T>(request: impl Future<Output = RedisResult<T>>) -> Result<T, String> {
    tokio::time::timeout(RESPONSE_TIMEOUT, request)
        .await
        .map_err(|_| "Valkey did not answer in time".to_string())?
        .map_err(|error| error.to_string())
}

/// Sleeps before the next attempt, settling subscriptions meanwhile so no
/// caller waits on a connection that is not there. False when the bus stopped.
async fn wait(
    delay: Duration,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    stop: &CancellationToken,
) -> bool {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            () = stop.cancelled() => return false,
            () = &mut sleep => return true,
            command = commands.recv() => match command {
                None => return false,
                Some(Command::Subscribe { confirmed, .. }) => {
                    confirmed.send_replace(true);
                }
                Some(Command::Unsubscribe { .. }) => {}
            },
        }
    }
}
