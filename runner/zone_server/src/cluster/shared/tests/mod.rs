use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use dashmap::DashMap;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::Shared;
use crate::cluster::clock::SKEW;
use crate::cluster::kind::Kind;
use crate::cluster::namespace::Namespace;
use crate::cluster::recording::Recording;
use crate::cluster::sealer::Sealer;
use crate::cluster::seen::Seen;
use crate::cluster::status::Status;
use crate::cluster::topic::Topic;
use crate::cluster::transport::Transport;
use crate::secret;

const PASSPHRASE: &str = "a-shared-bus-passphrase-of-at-least-32-bytes";

/// How long the last leave, part way through unsubscribing, waits for a join
/// racing it to finish.
const RACE: Duration = Duration::from_millis(300);

fn shared(recording: Recording) -> Shared {
    let master = secret::derive(PASSPHRASE).unwrap();
    let instance = Uuid::new_v4();
    let namespace = Namespace::derive(&master);
    Shared {
        instance,
        home: Topic::Instance(instance).channel(&namespace).into(),
        namespace,
        sealer: Sealer::derive(&master),
        channels: DashMap::new(),
        pending: DashMap::new(),
        seen: Mutex::new(Seen::new(Kind::Request.lifetime() + SKEW)),
        transport: Transport::Recording(recording),
        status: Arc::new(Status::new(None)),
        stop: CancellationToken::new().drop_guard(),
    }
}

fn recording(shared: &Shared) -> &Recording {
    match &shared.transport {
        Transport::Recording(recording) => recording,
        other => panic!("expected the recording transport, got {other:?}"),
    }
}

#[test]
fn a_join_racing_the_last_leave_leaves_the_channel_subscribed() {
    let (unsubscribing, unsubscribed) = mpsc::channel();
    let (joined, joining) = mpsc::channel::<()>();
    let joining = Mutex::new(joining);
    let shared = Arc::new(shared(Recording::new(move || {
        let _ = unsubscribing.send(());
        let _ = joining
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv_timeout(RACE);
    })));
    let topic = Topic::ChatFrames(Uuid::new_v4());
    let (channel, first, _) = shared.join(topic);
    drop(first);

    let leaving = {
        let shared = Arc::clone(&shared);
        let channel = Arc::clone(&channel);
        thread::spawn(move || shared.leave(&channel))
    };
    unsubscribed.recv().expect("the last leave unsubscribes");
    let rejoining = {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            let subscribed = shared.join(topic);
            let _ = joined.send(());
            subscribed
        })
    };
    leaving.join().expect("the leave ends");
    let (_, _receiver, confirmed) = rejoining.join().expect("the join ends");

    assert_eq!(
        recording(&shared).changes(),
        [
            format!("subscribe {channel}"),
            format!("unsubscribe {channel}"),
            format!("subscribe {channel}"),
        ],
        "Valkey hears the leave before the join that followed it, so the channel stays subscribed"
    );
    assert!(shared.channels.contains_key(&channel));
    assert!(*confirmed.borrow());
}

#[test]
fn only_the_last_subscriber_to_leave_unsubscribes() {
    let shared = shared(Recording::new(|| {}));
    let topic = Topic::ChatControl(Uuid::new_v4());
    let (channel, first, _) = shared.join(topic);
    let (_, second, _) = shared.join(topic);

    drop(first);
    shared.leave(&channel);
    let unsubscribed = recording(&shared).changes();
    drop(second);
    shared.leave(&channel);

    assert_eq!(unsubscribed, [format!("subscribe {channel}")]);
    assert_eq!(
        recording(&shared).changes(),
        [
            format!("subscribe {channel}"),
            format!("unsubscribe {channel}"),
        ]
    );
    assert!(!shared.channels.contains_key(&channel));
}
