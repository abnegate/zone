//! Chat turns across replicas.
//!
//! The replica running a turn publishes every frame once, serialized, on the
//! chat's frames topic. A replica with sockets on the chat but no turn of its
//! own (a mirror) forwards those bytes to them, keeps no log of the turn, and
//! asks the owner for a replay when a socket joins or when frames go missing.
//! Commands a mirror's sockets send reach the owner as requests on the chat's
//! control topic, which only the replica holding the chat answers. The
//! replica answering a chat says so under the chat's serving key, so the
//! others find it even while it holds a queue and no lease.
//!
//! One replica answers a chat at a time: the one its serving key names. An
//! answering task claims the key before it listens, and again as it takes
//! each request, so a request is never applied by a replica the key has
//! moved away from. A replica holding the chat keeps the key from every
//! other; one only waiting out its grace gives it up to one that holds the
//! chat. A turn that would start on a replica while another holds the chat is
//! deferred rather than run beside it.

mod acknowledgement;
mod answered;
mod beacon;
mod claim;
mod command;
mod floor;
mod header;
mod holding;
mod outcome;
mod persisted;
mod relayed;
mod replay;
mod request;
mod reservation;
mod route;
mod seen;
mod serving;
mod shown;
mod watch;

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use bytes::Bytes;
use chrono::{DateTime, TimeDelta, Utc};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::cluster::{Bus, Envelope, Subscription, Topic};
use crate::services::chat::session::Session;
use crate::state::AppState;

use super::{ChatStream, Frame, ServerMessage};

pub(super) use command::Command;
pub(super) use floor::Floor;
pub(super) use relayed::Relayed;
pub(super) use reservation::Reservation;
pub(super) use route::Route;
pub(super) use watch::Watch;

use acknowledgement::Acknowledgement;
use answered::Answered;
use beacon::Beacon;
use header::Header;
use holding::Holding;
use outcome::Outcome;
use replay::Replay;
use request::Request;
use seen::Seen;
use serving::Serving;

/// How long a replica waits for the owner to answer a command or a replay.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// How long before the asking replica stops waiting the owner stops applying
/// a command: time for the acknowledgement to arrive, and for the clocks of
/// the two replicas to disagree.
const ANSWER_MARGIN: Duration = Duration::from_millis(500);

/// How many times a replica asks the owner to apply one command before it
/// gives up on an answer.
const ATTEMPTS: usize = 2;

/// How many forwarded messages an answering task remembers having taken.
const SEEN_CAPACITY: usize = 1024;

/// How long the owner keeps answering after its last turn and queued message.
const GRACE: Duration = Duration::from_secs(5);

/// How often the owner checks whether it still has anything to answer for.
const IDLE_CHECK: Duration = Duration::from_millis(500);

/// How often a mirror reads the lease of a turn it has not seen end.
const WATCHDOG: Duration = Duration::from_secs(5);

/// Told to a socket whose command the replica holding the chat did not apply
/// by the command's deadline, and so never will.
pub(super) const BUSY: &str =
    "This chat is busy with a reply on another server. Try again in a moment.";

const ACCESS_DENIED: &str = "Access denied";

/// Each answering task's claim on its chat's serving key, by the bus instance
/// it answers as and the chat.
static SERVING: Lazy<DashMap<Answered, Arc<Mutex<Beacon>>>> = Lazy::new(DashMap::new);

/// Whether another replica can hear this one.
pub(super) fn reaches_peers(state: &AppState) -> bool {
    state.bus().connected()
}

/// Relays a frame just published here to every other replica.
pub(super) fn publish(state: &AppState, chat: Uuid, sequence: u64, message: &ServerMessage) {
    let json = match serde_json::to_vec(message) {
        Ok(json) => Bytes::from(json),
        Err(error) => {
            tracing::error!(%chat, %error, "Could not serialize a chat frame for the other replicas");
            return;
        }
    };
    let header = Header {
        chat,
        sequence,
        terminator: terminates(message),
    };
    state
        .bus()
        .publish_raw(Topic::ChatFrames(chat), &header, json);
}

fn terminates(message: &ServerMessage) -> bool {
    matches!(
        message,
        ServerMessage::MessageEnd { .. }
            | ServerMessage::Cancelled { .. }
            | ServerMessage::Error { .. }
            | ServerMessage::Denied { .. }
    )
}

/// Subscribes this replica to the frames other replicas publish to the chat,
/// once for the stream's life, and returns when the subscription is live.
pub(super) async fn listen(stream: &Arc<ChatStream>) {
    stream
        .listening
        .get_or_init(|| async {
            let Some(state) = stream.relay.get() else {
                return;
            };
            let subscription = state
                .bus()
                .subscribe(Topic::ChatFrames(stream.chat_id))
                .await;
            tokio::spawn(mirror(
                Arc::downgrade(stream),
                subscription,
                state.clone(),
                stream.stop.clone(),
            ));
        })
        .await;
}

async fn mirror(
    stream: Weak<ChatStream>,
    mut subscription: Subscription,
    state: AppState,
    stop: CancellationToken,
) {
    let mut tracked: HashMap<Uuid, u64> = HashMap::new();
    let mut watchdog = tokio::time::interval(WATCHDOG);
    watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    watchdog.tick().await;
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            received = subscription.recv() => {
                let Some(stream) = stream.upgrade() else {
                    return;
                };
                match received {
                    Ok(envelope) => forward(&stream, &mut tracked, &envelope).await,
                    Err(RecvError::Lagged(missed)) => {
                        tracing::warn!(chat = %stream.chat_id, missed, "Fell behind the frames another replica relays");
                        tracked.clear();
                        let _ = stream.events.send(Frame::Gap);
                    }
                    Err(RecvError::Closed) => return,
                }
            }
            _ = watchdog.tick() => {
                let Some(stream) = stream.upgrade() else {
                    return;
                };
                guard(&state, &stream).await;
            }
        }
    }
}

/// Hands a relayed frame to the local sockets. Each replica numbers the
/// frames it publishes to a chat, so a number skipped means frames were lost
/// and the sockets need a replay; a number already seen is a frame they have.
async fn forward(stream: &ChatStream, tracked: &mut HashMap<Uuid, u64>, envelope: &Envelope) {
    let header: Header = match envelope.header() {
        Ok(header) => header,
        Err(error) => {
            tracing::debug!(%error, "Dropped a chat frame without a readable header");
            return;
        }
    };
    if header.chat != stream.chat_id {
        return;
    }
    let origin = envelope.origin();
    let gap = match tracked.get(&origin) {
        Some(&last) if header.sequence <= last => return,
        Some(&last) => header.sequence > last + 1,
        None => false,
    };
    if header.terminator {
        tracked.remove(&origin);
    } else {
        tracked.insert(origin, header.sequence);
    }
    if !super::owns(stream.chat_id) {
        stream.watch.lock().await.observe(header.terminator);
    }
    if gap {
        let _ = stream.events.send(Frame::Gap);
    }
    let _ = stream.events.send(Frame::Relayed(Relayed {
        origin,
        sequence: header.sequence,
        json: envelope.bytes().clone(),
    }));
}

/// Ends, for this replica's sockets, a relayed turn whose lease is gone.
async fn guard(state: &AppState, stream: &ChatStream) {
    let mut watch = stream.watch.lock().await;
    if super::owns(stream.chat_id) {
        watch.clear();
        return;
    }
    if let Some(closing) = watch.check(state.db(), stream.chat_id).await {
        stream.local(closing);
    }
}

/// Ends a relayed turn this replica is about to start a turn after, so its
/// sockets never see the new turn begin before the old one ended.
pub(super) async fn settle(stream: &ChatStream) {
    if let Some(state) = stream.relay.get() {
        guard(state, stream).await;
    }
}

/// The turn in flight on another replica and the queue it holds, for a socket
/// joining. None when this replica runs the chat, or when no lease is live
/// and no other replica holds the chat, so there is nothing to wait for. A
/// live lease nobody answers for still arms the watchdog.
pub(super) async fn replay(state: &AppState, stream: &ChatStream) -> Option<(Vec<String>, Floor)> {
    let chat = stream.chat_id;
    if !reaches_peers(state) || super::owns(chat) {
        return None;
    }
    let holding = Holding::live(state.db(), chat).await;
    if holding.is_none() && !held_elsewhere(state, chat).await {
        return None;
    }
    let replay = ask(state, chat).await;
    if let Some(holding) = holding {
        stream.watch.lock().await.arm(holding.fence);
    }
    replay.map(|replay| (replay.frames, replay.floor))
}

async fn ask(state: &AppState, chat: Uuid) -> Option<Replay> {
    let envelope = state
        .bus()
        .request(Topic::ChatControl(chat), &Request::Replay, ANSWER_TIMEOUT)
        .await?;
    let acknowledgement: Acknowledgement = envelope
        .payload()
        .inspect_err(|error| tracing::warn!(%chat, %error, "Dropped an unreadable replay"))
        .ok()?;
    Some(Replay {
        frames: acknowledgement.frames,
        floor: Floor {
            origin: envelope.origin(),
            sequence: acknowledgement.sequence,
        },
    })
}

/// Whether another replica's serving key says it holds `chat`: runs its turn
/// or keeps messages queued for one.
async fn held_elsewhere(state: &AppState, chat: Uuid) -> bool {
    Serving::elsewhere(state.bus(), chat)
        .await
        .is_some_and(|serving| serving.holding)
}

/// Sends a socket's command to the replica serving the chat, or says this
/// replica should apply it. A command is forwarded while another replica
/// holds the chat's lease or says it serves the chat, and asked again, as
/// itself, when the first try goes unanswered. Each try carries a deadline
/// the owner applies it by or never, so once every try is unanswered the chat
/// is busy while another replica still holds it, by its lease or by a serving
/// key saying it keeps the chat's queue, and is left to this replica otherwise.
pub(super) async fn route(
    state: &AppState,
    chat: Uuid,
    user: Uuid,
    session: Uuid,
    command: &Command,
) -> Route {
    if !reaches_peers(state) || super::owns(chat) {
        return Route::Local;
    }
    if Holding::live(state.db(), chat).await.is_none()
        && Serving::elsewhere(state.bus(), chat).await.is_none()
    {
        return Route::Local;
    }
    for _ in 0..ATTEMPTS {
        let Some(acknowledgement) = attempt(state, chat, user, session, command).await else {
            continue;
        };
        match acknowledgement.outcome {
            Outcome::Applied | Outcome::Duplicate | Outcome::Refused => {
                return Route::Forwarded(acknowledgement.frames);
            }
            Outcome::Expired => {}
        }
    }
    if Holding::live(state.db(), chat).await.is_some() || held_elsewhere(state, chat).await {
        return Route::Busy;
    }
    Route::Local
}

/// Asks the replica serving the chat to apply `command` once.
async fn attempt(
    state: &AppState,
    chat: Uuid,
    user: Uuid,
    session: Uuid,
    command: &Command,
) -> Option<Acknowledgement> {
    let request = Request::Control {
        user,
        session,
        deadline: deadline(Utc::now()),
        command: command.clone(),
    };
    let envelope = state
        .bus()
        .request(Topic::ChatControl(chat), &request, ANSWER_TIMEOUT)
        .await?;
    envelope
        .payload()
        .inspect_err(|error| tracing::warn!(%chat, %error, "Dropped an unreadable acknowledgement"))
        .ok()
}

/// When the owner must have applied a command asked at `asked`, for its
/// answer to reach the asking replica before that stops waiting.
fn deadline(asked: DateTime<Utc>) -> DateTime<Utc> {
    let window = ANSWER_TIMEOUT.saturating_sub(ANSWER_MARGIN);
    asked + TimeDelta::from_std(window).unwrap_or(TimeDelta::zero())
}

fn expired(deadline: DateTime<Utc>) -> bool {
    Utc::now() > deadline
}

/// Answers the chat's control topic while this replica runs or queues a turn
/// for it, and for a grace period after. Returns once the subscription is
/// live, so a turn started next cannot be asked about before it can answer.
/// Says whether this replica answers the chat: false when another replica
/// holds it, so a turn about to start here should not.
pub(super) async fn serve(state: &AppState, stream: &Arc<ChatStream>, workspace: Uuid) -> bool {
    let chat = stream.chat_id;
    let holding = super::owns(chat);
    let answered = Answered {
        instance: state.bus().instance(),
        chat,
    };
    let beacon = match SERVING.entry(answered) {
        Entry::Occupied(occupied) => {
            let beacon = Arc::clone(occupied.get());
            drop(occupied);
            return beacon.lock().await.claim(holding).await;
        }
        Entry::Vacant(vacant) => {
            let beacon = Arc::new(Mutex::new(Beacon::new(state.bus().clone(), chat)));
            vacant.insert(Arc::clone(&beacon));
            beacon
        }
    };
    let answers = beacon.lock().await.claim(holding).await;
    let subscription = state.bus().subscribe(Topic::ChatControl(chat)).await;
    tokio::spawn(answering(
        state.clone(),
        Arc::clone(stream),
        workspace,
        subscription,
        beacon,
    ));
    answers
}

/// Takes down every serving key this replica wrote over `bus`, so the chats
/// it answered pass to another replica as it stops rather than once the keys
/// lapse.
pub(super) async fn withdraw(bus: &Bus) {
    let instance = bus.instance();
    let beacons: Vec<Arc<Mutex<Beacon>>> = SERVING
        .iter()
        .filter(|serving| serving.key().instance == instance)
        .map(|serving| Arc::clone(serving.value()))
        .collect();
    for beacon in beacons {
        beacon.lock().await.hide().await;
    }
}

async fn answering(
    state: AppState,
    stream: Arc<ChatStream>,
    workspace: Uuid,
    mut subscription: Subscription,
    beacon: Arc<Mutex<Beacon>>,
) {
    let chat = stream.chat_id;
    let key = Answered {
        instance: state.bus().instance(),
        chat,
    };
    let displaced = Arc::new(Notify::new());
    let stop = CancellationToken::new();
    let shining = tokio::spawn(shine(
        state.clone(),
        chat,
        Arc::clone(&beacon),
        Arc::clone(&displaced),
        stop.clone(),
    ));
    let mine = |serving: &Arc<Mutex<Beacon>>| Arc::ptr_eq(serving, &beacon);
    let mut seen = Seen::new(SEEN_CAPACITY);
    let mut idle_since: Option<Instant> = None;
    let mut check = tokio::time::interval(IDLE_CHECK);
    loop {
        tokio::select! {
            received = subscription.recv() => match received {
                Ok(envelope) => answer(&state, &stream, workspace, &beacon, &mut seen, &envelope).await,
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(%chat, missed, "Missed commands forwarded by other replicas");
                }
                Err(RecvError::Closed) => break,
            },
            () = displaced.notified() => {
                if SERVING
                    .remove_if(&key, |_, serving| mine(serving) && !super::owns(chat))
                    .is_some()
                {
                    break;
                }
            }
            _ = check.tick() => {
                if super::owns(chat) {
                    idle_since = None;
                    continue;
                }
                let since = *idle_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= GRACE
                    && SERVING
                        .remove_if(&key, |_, serving| mine(serving) && !super::owns(chat))
                        .is_some()
                {
                    break;
                }
            }
        }
    }
    SERVING.remove_if(&key, |_, serving| mine(serving));
    stop.cancel();
    let _ = shining.await;
    beacon.lock().await.hide().await;
}

/// Keeps an answering task's claim on its chat fresh, apart from the task, so
/// a request it takes long to apply never lets the claim lapse. Tells the task
/// when another replica's answerer has the chat and this replica holds
/// nothing, so it has nothing left to answer for.
async fn shine(
    state: AppState,
    chat: Uuid,
    beacon: Arc<Mutex<Beacon>>,
    displaced: Arc<Notify>,
    stop: CancellationToken,
) {
    let mut check = tokio::time::interval(IDLE_CHECK);
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            _ = check.tick() => {
                let holding = super::owns(chat);
                let mut beacon = beacon.lock().await;
                if state.drain().is_draining() {
                    beacon.hide().await;
                } else if !beacon.show(holding).await && !holding {
                    displaced.notify_one();
                }
            }
        }
    }
}

/// Applies one forwarded request, answering only as the replica the chat's
/// serving key names. That replica replays what it has to a joining socket,
/// an ended turn's nothing included, so the socket never waits out a replay
/// a stale key asked for. It applies a command only as the replica holding
/// the chat or the one that already took the Send. A replica in its grace period
/// holds nothing, so it takes a forwarded Send only by taking the chat's lease
/// first, and stays silent when another replica has it or fleet policy refuses
/// this runner the chat. Nothing is applied past the request's deadline.
async fn answer(
    state: &AppState,
    stream: &Arc<ChatStream>,
    workspace: Uuid,
    beacon: &Mutex<Beacon>,
    seen: &mut Seen,
    envelope: &Envelope,
) {
    let chat = stream.chat_id;
    let request: Request = match envelope.payload() {
        Ok(request) => request,
        Err(error) => {
            tracing::debug!(%chat, %error, "Dropped an unreadable chat request");
            return;
        }
    };
    let draining = state.drain().is_draining();
    if !draining && !beacon.lock().await.claim(super::owns(chat)).await {
        return;
    }
    let (user, session, deadline, command) = match request {
        Request::Replay => {
            if draining && !super::owns(chat) {
                return;
            }
            let (frames, sequence) = stream.snapshot().await;
            acknowledge(state, envelope, Outcome::Applied, sequence, &frames);
            return;
        }
        Request::Control {
            user,
            session,
            deadline,
            command,
        } => (user, session, deadline, command),
    };
    let sent = match &command {
        Command::Send { id, .. } => Some(*id),
        _ => None,
    };
    if sent.is_some_and(|id| seen.contains(&id)) {
        acknowledge(state, envelope, Outcome::Duplicate, stream.sequence(), &[]);
        return;
    }
    if draining && matches!(command, Command::Send { .. } | Command::Interrupt { .. }) {
        return;
    }
    let holder = super::owns(chat);
    if !holder && sent.is_none() {
        return;
    }
    if expired(deadline) {
        if holder {
            acknowledge(state, envelope, Outcome::Expired, stream.sequence(), &[]);
        }
        return;
    }
    let reserved = !holder;
    if reserved {
        match Session::acquire(state, chat, workspace, Uuid::new_v4()).await {
            Ok(session) => Reservation::hold(chat, session),
            Err(crate::db::context::Error::Busy) => return,
            Err(crate::db::context::Error::Refused(shortfall)) => {
                tracing::info!(
                    %chat,
                    ?shortfall,
                    "Fleet policy refused this runner a forwarded message, so the replica that asked runs it"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(%chat, %error, "Could not take the chat for a forwarded message");
                return;
            }
        }
    }
    if !super::can_access_chat(state, user, session, workspace)
        .await
        .unwrap_or(false)
    {
        if reserved {
            drop(Reservation::claim(chat));
        }
        let refusal = ServerMessage::Error {
            message: ACCESS_DENIED.to_string(),
        };
        acknowledge(
            state,
            envelope,
            Outcome::Refused,
            stream.sequence(),
            &[refusal],
        );
        return;
    }
    if expired(deadline) {
        if reserved {
            drop(Reservation::claim(chat));
        } else {
            acknowledge(state, envelope, Outcome::Expired, stream.sequence(), &[]);
        }
        return;
    }
    let handled = super::control(state, stream, workspace, user, command).await;
    if reserved && !handled.started {
        drop(Reservation::claim(chat));
    }
    if let Some(id) = sent {
        seen.insert(id);
    }
    acknowledge(
        state,
        envelope,
        Outcome::Applied,
        stream.sequence(),
        &handled.frames,
    );
}

fn acknowledge(
    state: &AppState,
    request: &Envelope,
    outcome: Outcome,
    sequence: u64,
    frames: &[ServerMessage],
) {
    let frames = frames
        .iter()
        .filter_map(|frame| {
            serde_json::to_string(frame)
                .inspect_err(|error| tracing::error!(%error, "Could not serialize a chat frame"))
                .ok()
        })
        .collect();
    state.bus().reply(
        request,
        &Acknowledgement {
            outcome,
            sequence,
            frames,
        },
    );
}

#[cfg(test)]
mod tests {
    use metrics::Gauge;

    use super::super::queue::{self, Finished, Queue, Queued};
    use super::*;
    use crate::secret;
    use crate::state::{test_config, unreachable_pool};

    /// How long another replica's claim on a chat lasts in a test.
    const HELD: Duration = Duration::from_secs(30);

    /// How long a bus may take to reach Valkey, and a refusal to arrive.
    const WAIT: Duration = Duration::from_secs(10);

    /// A bus on the Valkey at `REDIS_URL`, as one replica of a cluster sharing `passphrase`.
    async fn replica(passphrase: &str) -> Bus {
        let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
        let bus = Bus::connect(&url, &secret::derive(passphrase).unwrap(), Gauge::noop());
        tokio::time::timeout(WAIT, bus.connection().wait_for(|connected| *connected))
            .await
            .expect("the bus reaches Valkey at REDIS_URL")
            .expect("the bus status is watched");
        bus
    }

    /// The first error published to `chat`'s sockets on this replica.
    async fn refusal(events: &mut tokio::sync::broadcast::Receiver<Frame>) -> String {
        loop {
            match events.recv().await {
                Ok(Frame::Message(message)) => {
                    if let ServerMessage::Error { message } = &*message {
                        return message.clone();
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => panic!("the chat's frames closed"),
            }
        }
    }

    /// Says, as `holder`, that another replica holds `chat`, as one that took
    /// its serving key while this replica could not reach Valkey would. Gives
    /// back what it wrote, to take down again.
    async fn hold(holder: &Bus, chat: Uuid) -> String {
        let held = Serving {
            instance: holder.instance(),
            answerer: Uuid::new_v4(),
            holding: true,
        };
        match held.claim(holder, chat, HELD).await {
            claim::Claim::Taken(payload) => payload,
            other => panic!("the other replica takes the chat's serving key, got {other:?}"),
        }
    }

    fn queued(content: &str) -> Queued {
        Queued {
            id: Uuid::new_v4(),
            content: content.to_string(),
            metadata: None,
            author: Uuid::new_v4(),
        }
    }

    fn ids(messages: &[Queued]) -> Vec<Uuid> {
        messages.iter().map(|message| message.id).collect()
    }

    /// The queue this replica's sockets are shown once they are told the
    /// chat is busy: its messages and whether it is held.
    async fn refused(events: &mut tokio::sync::broadcast::Receiver<Frame>) -> (Vec<Uuid>, bool) {
        let mut busy = false;
        loop {
            match events.recv().await {
                Ok(Frame::Message(message)) => match &*message {
                    ServerMessage::Error { message } if message == BUSY => busy = true,
                    ServerMessage::Queue { messages, held, .. } if busy => {
                        return (ids(messages), *held);
                    }
                    _ => {}
                },
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => panic!("the chat's frames closed"),
            }
        }
    }

    /// The message the next queue frame here says left the queue to start.
    async fn started(events: &mut tokio::sync::broadcast::Receiver<Frame>) -> Uuid {
        loop {
            match events.recv().await {
                Ok(Frame::Message(message)) => {
                    if let ServerMessage::Queue {
                        started: Some(started),
                        ..
                    } = &*message
                    {
                        return started.id;
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => panic!("the chat's frames closed"),
            }
        }
    }

    /// The queue a socket joining this replica is shown.
    async fn joined(stream: &ChatStream) -> Option<(Vec<Uuid>, bool)> {
        let (frames, _) = stream.snapshot().await;
        frames.into_iter().find_map(|frame| match frame {
            ServerMessage::Queue { messages, held, .. } => Some((ids(&messages), held)),
            _ => None,
        })
    }

    fn queue_of(chat: Uuid) -> Option<(Vec<Uuid>, bool)> {
        queue::snapshot(chat).map(|(messages, held)| (ids(&messages), held))
    }

    /// A queue held here whose chat another replica took, as one does once
    /// this replica has been cut off from Valkey longer than a serving key
    /// lives, loses nothing when an Interrupt is refused busy: every held
    /// message stays queued, shown to the sockets here, and runs once the
    /// other replica lets the chat go.
    #[tokio::test]
    async fn an_interrupt_refused_while_another_replica_took_the_chat_keeps_every_held_message() {
        let passphrase = format!("relay-interrupt-{}", Uuid::new_v4().simple());
        let (here, holder) = (replica(&passphrase).await, replica(&passphrase).await);
        let chat = Uuid::new_v4();
        let (first, second) = (queued("first"), queued("second"));
        queue::with(chat, |queue| {
            queue.submit(first.clone(), true);
            queue.submit(second.clone(), true);
            queue.hold();
        });
        let taken = hold(&holder, chat).await;
        let state = AppState::new(test_config(), unreachable_pool(), None).with_bus(here);
        let stream = ChatStream::on(&state, chat);
        let mut events = stream.events.subscribe();
        let workspace = Uuid::new_v4();
        let interrupt = |id: Uuid| {
            super::super::control(
                &state,
                &stream,
                workspace,
                Uuid::new_v4(),
                Command::Interrupt { id },
            )
        };

        let handled = interrupt(second.id).await;
        let shown = tokio::time::timeout(WAIT, refused(&mut events))
            .await
            .expect("the interrupt is refused busy with the queue left here");

        assert!(handled.started, "the interrupt started the message here");
        let kept = (vec![second.id, first.id], true);
        assert_eq!(shown, kept, "the sockets here are shown every held message");
        assert_eq!(
            queue_of(chat),
            Some(kept.clone()),
            "the queue keeps every held message, the interrupted one first"
        );
        assert_eq!(joined(&stream).await, Some(kept));
        assert!(super::super::owns(chat), "the held queue stays here");

        Serving::erase(&holder, chat, &taken).await;
        interrupt(first.id).await;
        let ran = tokio::time::timeout(WAIT, async {
            [started(&mut events).await, started(&mut events).await]
        })
        .await
        .expect("the held messages run once the chat is free");
        assert_eq!(ran, [first.id, second.id]);
    }

    /// A queue waiting here behind another turn, released as that turn ends
    /// while another replica took the chat, is held rather than dropped, and
    /// the sockets here are shown it.
    #[tokio::test]
    async fn a_queue_released_while_another_replica_took_the_chat_is_held_with_every_message() {
        let passphrase = format!("relay-release-{}", Uuid::new_v4().simple());
        let (here, holder) = (replica(&passphrase).await, replica(&passphrase).await);
        let chat = Uuid::new_v4();
        let (first, second) = (queued("first"), queued("second"));
        queue::with(chat, |queue| {
            queue.submit(first.clone(), true);
            queue.submit(second.clone(), true);
        });
        hold(&holder, chat).await;
        let state = AppState::new(test_config(), unreachable_pool(), None).with_bus(here);
        let stream = ChatStream::on(&state, chat);
        let mut events = stream.events.subscribe();
        let workspace = Uuid::new_v4();
        let Finished::Next(next) = queue::with(chat, Queue::release) else {
            panic!("the queue starts its front once the other turn ends");
        };

        super::super::drive(
            state.clone(),
            Arc::clone(&stream),
            chat,
            workspace,
            super::super::begin(chat, workspace, next),
            true,
        );
        let shown = tokio::time::timeout(WAIT, refused(&mut events))
            .await
            .expect("the released queue is refused busy");

        let kept = (vec![first.id, second.id], true);
        assert_eq!(shown, kept);
        assert_eq!(queue_of(chat), Some(kept.clone()));
        assert_eq!(joined(&stream).await, Some(kept));
    }

    /// Messages queued here behind a send that started at once, while it
    /// waited to hear whether this replica serves the chat, stay queued and
    /// shown when that send is refused busy. Only the send that never showed
    /// in the queue is dropped.
    #[tokio::test]
    async fn messages_queued_behind_a_send_refused_busy_stay_queued_and_shown() {
        let passphrase = format!("relay-behind-{}", Uuid::new_v4().simple());
        let (here, holder) = (replica(&passphrase).await, replica(&passphrase).await);
        let chat = Uuid::new_v4();
        hold(&holder, chat).await;
        let state = AppState::new(test_config(), unreachable_pool(), None).with_bus(here);
        let stream = ChatStream::on(&state, chat);
        let mut events = stream.events.subscribe();
        let workspace = Uuid::new_v4();
        let key = Answered {
            instance: state.bus().instance(),
            chat,
        };
        let beacon = Arc::new(Mutex::new(Beacon::new(state.bus().clone(), chat)));
        SERVING.insert(key, Arc::clone(&beacon));
        let claiming = beacon.lock().await;
        let send = |message: Queued| {
            super::super::control(
                &state,
                &stream,
                workspace,
                message.author,
                Command::Send {
                    id: message.id,
                    content: message.content,
                    metadata: None,
                },
            )
        };
        let (sent, first, second) = (queued("sent"), queued("first"), queued("second"));

        assert!(send(sent).await.started, "the first send starts at once");
        assert!(!send(first.clone()).await.started);
        assert!(!send(second.clone()).await.started);
        drop(claiming);
        let shown = tokio::time::timeout(WAIT, refused(&mut events))
            .await
            .expect("the send is refused busy");
        SERVING.remove(&key);

        let kept = (vec![first.id, second.id], true);
        assert_eq!(shown, kept, "the sockets are shown what is still queued");
        assert_eq!(queue_of(chat), Some(kept));
    }

    /// A Send this replica took to run itself, because no other replica
    /// seemed to hold the chat when it was routed, is refused as busy rather
    /// than run beside the replica whose serving key holds the chat, and leaves
    /// nothing queued or running here.
    #[tokio::test]
    async fn a_send_started_here_while_another_replica_holds_the_chat_is_refused_busy() {
        let passphrase = format!("relay-drive-{}", Uuid::new_v4().simple());
        let (here, holder) = (replica(&passphrase).await, replica(&passphrase).await);
        let chat = Uuid::new_v4();
        let held = Serving {
            instance: holder.instance(),
            answerer: Uuid::new_v4(),
            holding: true,
        };
        assert!(matches!(
            held.claim(&holder, chat, HELD).await,
            claim::Claim::Taken(_)
        ));
        let state = AppState::new(test_config(), unreachable_pool(), None).with_bus(here);
        let stream = ChatStream::on(&state, chat);
        let mut events = stream.events.subscribe();

        let handled = super::super::control(
            &state,
            &stream,
            Uuid::new_v4(),
            Uuid::new_v4(),
            Command::Send {
                id: Uuid::new_v4(),
                content: "Hello".to_string(),
                metadata: None,
            },
        )
        .await;
        let refused = tokio::time::timeout(WAIT, refusal(&mut events))
            .await
            .expect("the send is refused");
        let settled = Instant::now() + WAIT;
        while super::super::owns(chat) && Instant::now() < settled {
            tokio::time::sleep(IDLE_CHECK).await;
        }

        assert!(handled.started, "the send left the queue to start here");
        assert_eq!(refused, BUSY);
        assert!(
            !super::super::owns(chat),
            "the refused send left nothing queued or running here"
        );
    }

    /// A replica stopping takes its serving keys down as it goes, rather than
    /// leaving them to lapse after it has gone, so the others never wait on
    /// or report busy a chat nobody answers.
    #[tokio::test]
    async fn a_replica_stopping_takes_down_its_claim_on_every_chat_it_answers() {
        let passphrase = format!("relay-withdraw-{}", Uuid::new_v4().simple());
        let (here, other) = (replica(&passphrase).await, replica(&passphrase).await);
        let state = AppState::new(test_config(), unreachable_pool(), None).with_bus(here);
        let chat = Uuid::new_v4();
        let stream = ChatStream::on(&state, chat);
        assert!(serve(&state, &stream, Uuid::new_v4()).await);
        let served = Serving::elsewhere(&other, chat).await;

        state.drain().begin();
        withdraw(state.bus()).await;

        assert_eq!(
            served.map(|serving| serving.instance),
            Some(state.bus().instance())
        );
        assert_eq!(Serving::elsewhere(&other, chat).await, None);
    }

    /// A replica stopping leaves alone the claims every other replica keeps,
    /// one sharing its process included, and never waits on them.
    #[tokio::test]
    async fn a_replica_stopping_leaves_the_claims_of_every_other_replica() {
        let passphrase = format!("relay-neighbour-{}", Uuid::new_v4().simple());
        let (here, there, other) = (
            replica(&passphrase).await,
            replica(&passphrase).await,
            replica(&passphrase).await,
        );
        let state = AppState::new(test_config(), unreachable_pool(), None).with_bus(here);
        let neighbour = AppState::new(test_config(), unreachable_pool(), None).with_bus(there);
        let chat = Uuid::new_v4();
        let stream = ChatStream::on(&neighbour, chat);
        assert!(serve(&neighbour, &stream, Uuid::new_v4()).await);
        let beacon = SERVING
            .get(&Answered {
                instance: neighbour.bus().instance(),
                chat,
            })
            .map(|serving| Arc::clone(serving.value()))
            .expect("the neighbour answers its chat");
        let untouched = beacon.lock().await;

        state.drain().begin();
        tokio::time::timeout(WAIT, withdraw(state.bus()))
            .await
            .expect("withdrawing never waits on another replica's claim");
        let served = Serving::elsewhere(&other, chat).await;
        drop(untouched);

        assert_eq!(
            served.map(|serving| serving.instance),
            Some(neighbour.bus().instance()),
            "the neighbour still answers its chat"
        );
    }

    #[test]
    fn only_the_frames_that_end_a_turn_are_terminators() {
        let id = Uuid::new_v4();
        for (message, expected) in [
            (
                ServerMessage::MessageEnd {
                    message_id: id,
                    content: String::new(),
                    metadata: None,
                    error: None,
                },
                true,
            ),
            (ServerMessage::Cancelled { message_id: None }, true),
            (
                ServerMessage::Error {
                    message: String::new(),
                },
                true,
            ),
            (
                ServerMessage::Denied {
                    message: String::new(),
                    code: String::new(),
                },
                true,
            ),
            (
                ServerMessage::Chunk {
                    content: String::new(),
                    index: 0,
                },
                false,
            ),
            (
                ServerMessage::MessageStart {
                    message_id: id,
                    role: "assistant".to_string(),
                    resumed: false,
                },
                false,
            ),
        ] {
            assert_eq!(terminates(&message), expected, "{message:?}");
        }
    }

    #[test]
    fn a_floor_covers_its_own_replica_up_to_its_sequence() {
        let origin = Uuid::new_v4();
        let floor = Floor {
            origin,
            sequence: 7,
        };
        let frame = |origin, sequence| Relayed {
            origin,
            sequence,
            json: Bytes::new(),
        };

        assert!(floor.covers(&frame(origin, 7)));
        assert!(floor.covers(&frame(origin, 3)));
        assert!(!floor.covers(&frame(origin, 8)));
        assert!(!floor.covers(&frame(Uuid::new_v4(), 3)));
    }

    #[test]
    fn requests_and_acknowledgements_survive_the_bus() {
        let request = Request::Control {
            user: Uuid::new_v4(),
            session: Uuid::new_v4(),
            deadline: deadline(Utc::now()),
            command: Command::Send {
                id: Uuid::new_v4(),
                content: "Hello".to_string(),
                metadata: Some(serde_json::json!({"source": "console"})),
            },
        };
        let acknowledgement = Acknowledgement {
            outcome: Outcome::Duplicate,
            sequence: 9,
            frames: vec![r#"{"type":"tool_approval_closed","tool_call_id":"call"}"#.to_string()],
        };

        let encoded = serde_json::to_vec(&request).unwrap();
        assert_eq!(
            serde_json::from_slice::<Request>(&encoded).unwrap(),
            request
        );
        let encoded = serde_json::to_vec(&acknowledgement).unwrap();
        assert_eq!(
            serde_json::from_slice::<Acknowledgement>(&encoded).unwrap(),
            acknowledgement
        );
        let replay = serde_json::to_vec(&Request::Replay).unwrap();
        assert_eq!(
            serde_json::from_slice::<Request>(&replay).unwrap(),
            Request::Replay
        );
    }

    #[test]
    fn the_owner_stops_applying_a_command_before_the_asker_stops_waiting() {
        let asked = Utc::now();

        let due = deadline(asked);

        assert!(due > asked);
        assert_eq!(
            (asked + TimeDelta::from_std(ANSWER_TIMEOUT).unwrap()) - due,
            TimeDelta::from_std(ANSWER_MARGIN).unwrap()
        );
        assert!(!expired(Utc::now() + TimeDelta::seconds(1)));
        assert!(expired(Utc::now() - TimeDelta::seconds(1)));
    }
}
