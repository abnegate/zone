//! Chat turns across replicas.
//!
//! The replica running a turn publishes every frame once, serialized, on the
//! chat's frames topic. A replica with sockets on the chat but no turn of its
//! own (a mirror) forwards those bytes to them, keeps no log of the turn, and
//! asks the owner for a replay when a socket joins or when frames go missing.
//! Commands a mirror's sockets send reach the owner as requests on the chat's
//! control topic, which only the replica holding the chat answers.

mod acknowledgement;
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
mod watch;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use bytes::Bytes;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use once_cell::sync::Lazy;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::cluster::{Envelope, Subscription, Topic};
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
use header::Header;
use holding::Holding;
use outcome::Outcome;
use replay::Replay;
use request::Request;

/// How long a replica waits for the owner to answer a command or a replay.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the owner keeps answering after its last turn and queued message.
const GRACE: Duration = Duration::from_secs(5);

/// How often the owner checks whether it still has anything to answer for.
const IDLE_CHECK: Duration = Duration::from_millis(500);

/// How often a mirror reads the lease of a turn it has not seen end.
const WATCHDOG: Duration = Duration::from_secs(5);

/// Told to a socket whose command the replica holding the chat did not answer.
pub(super) const BUSY: &str =
    "This chat is busy with a reply on another server. Try again in a moment.";

const ACCESS_DENIED: &str = "Access denied";

/// The replica answering each chat's control topic, by the id of its answering task.
static SERVING: Lazy<DashMap<Uuid, Uuid>> = Lazy::new(DashMap::new);

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

/// The turn in flight on another replica, for a socket joining it. None when
/// this replica runs the chat or no lease is live, so there is nothing to
/// wait for. A live lease nobody answers for still arms the watchdog.
pub(super) async fn replay(state: &AppState, stream: &ChatStream) -> Option<(Vec<String>, Floor)> {
    let chat = stream.chat_id;
    if !reaches_peers(state) || super::owns(chat) {
        return None;
    }
    let holding = Holding::live(state.db(), chat).await?;
    let replay = ask(state, chat).await;
    stream.watch.lock().await.arm(holding.fence);
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

/// Sends a socket's command to the replica running the chat's turn, or says
/// this replica should apply it. With no answer, the lease decides: a lease
/// that lapsed leaves the chat to this replica, and a live one means busy.
pub(super) async fn route(
    state: &AppState,
    chat: Uuid,
    user: Uuid,
    session: Uuid,
    command: &Command,
) -> Route {
    if !reaches_peers(state) || super::owns(chat) || Holding::live(state.db(), chat).await.is_none()
    {
        return Route::Local;
    }
    let request = Request::Control {
        user,
        session,
        command: command.clone(),
    };
    if let Some(envelope) = state
        .bus()
        .request(Topic::ChatControl(chat), &request, ANSWER_TIMEOUT)
        .await
    {
        match envelope.payload::<Acknowledgement>() {
            Ok(acknowledgement) => return Route::Forwarded(acknowledgement),
            Err(error) => tracing::warn!(%chat, %error, "Dropped an unreadable acknowledgement"),
        }
    }
    match Holding::live(state.db(), chat).await {
        Some(_) => Route::Busy,
        None => Route::Local,
    }
}

/// Answers the chat's control topic while this replica runs or queues a turn
/// for it, and for a grace period after. Returns once the subscription is
/// live, so a turn started next cannot be asked about before it can answer.
pub(super) async fn serve(state: &AppState, stream: &Arc<ChatStream>, workspace: Uuid) {
    let chat = stream.chat_id;
    let id = Uuid::new_v4();
    match SERVING.entry(chat) {
        Entry::Occupied(_) => return,
        Entry::Vacant(vacant) => {
            vacant.insert(id);
        }
    }
    let subscription = state.bus().subscribe(Topic::ChatControl(chat)).await;
    tokio::spawn(answering(
        state.clone(),
        Arc::clone(stream),
        workspace,
        subscription,
        id,
    ));
}

async fn answering(
    state: AppState,
    stream: Arc<ChatStream>,
    workspace: Uuid,
    mut subscription: Subscription,
    id: Uuid,
) {
    let chat = stream.chat_id;
    let mut seen = HashSet::new();
    let mut idle_since: Option<Instant> = None;
    let mut check = tokio::time::interval(IDLE_CHECK);
    loop {
        tokio::select! {
            received = subscription.recv() => match received {
                Ok(envelope) => answer(&state, &stream, workspace, &mut seen, &envelope).await,
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(%chat, missed, "Missed commands forwarded by other replicas");
                }
                Err(RecvError::Closed) => break,
            },
            _ = check.tick() => {
                if super::owns(chat) {
                    idle_since = None;
                    continue;
                }
                let since = *idle_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= GRACE
                    && SERVING
                        .remove_if(&chat, |_, serving| *serving == id && !super::owns(chat))
                        .is_some()
                {
                    break;
                }
            }
        }
    }
    SERVING.remove_if(&chat, |_, serving| *serving == id);
}

/// Applies one forwarded request, answering only as the replica holding the
/// chat. A replica in its grace period holds nothing, so it takes a forwarded
/// Send only by taking the chat's lease first, and stays silent when another
/// replica has it.
async fn answer(
    state: &AppState,
    stream: &Arc<ChatStream>,
    workspace: Uuid,
    seen: &mut HashSet<Uuid>,
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
    let (user, session, command) = match request {
        Request::Replay => {
            if super::owns(chat) {
                let (frames, sequence) = stream.snapshot().await;
                acknowledge(state, envelope, Outcome::Applied, sequence, &frames);
            }
            return;
        }
        Request::Control {
            user,
            session,
            command,
        } => (user, session, command),
    };
    let holder = super::owns(chat);
    let sent = match &command {
        Command::Send { id, .. } => Some(*id),
        _ => None,
    };
    if !holder && sent.is_none() {
        return;
    }
    if sent.is_some_and(|id| seen.contains(&id)) {
        if holder {
            acknowledge(state, envelope, Outcome::Duplicate, stream.sequence(), &[]);
        }
        return;
    }
    let reserved = !holder;
    if reserved {
        match Session::acquire(state, chat, workspace, Uuid::new_v4()).await {
            Ok(session) => Reservation::hold(chat, session),
            Err(crate::db::context::Error::Busy) => return,
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
    use super::*;

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
}
