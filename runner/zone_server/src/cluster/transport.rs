use std::sync::Arc;

use redis::aio::ConnectionManager;
use tokio::sync::watch;

use super::command::Command;
use super::loopback::Loopback;
use super::packet::Packet;
#[cfg(test)]
use super::recording::Recording;
use super::valkey::Valkey;

/// How envelopes leave this process and reach the others.
#[derive(Debug)]
pub enum Transport {
    Loopback(Loopback),
    Valkey(Valkey),
    #[cfg(test)]
    Recording(Recording),
}

impl Transport {
    /// Whether an envelope published now could reach another instance, so a
    /// lone loopback bus skips sealing envelopes nobody would hear.
    pub fn reaches_peers(&self) -> bool {
        match self {
            Self::Loopback(loopback) => loopback.has_peers(),
            Self::Valkey(_) => true,
            #[cfg(test)]
            Self::Recording(_) => false,
        }
    }

    pub fn valkey(&self) -> Option<&ConnectionManager> {
        match self {
            Self::Valkey(valkey) => Some(valkey.manager()),
            _ => None,
        }
    }

    pub fn send(&self, packet: Packet) {
        match self {
            Self::Loopback(loopback) => loopback.send(packet),
            Self::Valkey(valkey) => valkey.send(packet),
            #[cfg(test)]
            Self::Recording(_) => {}
        }
    }

    pub fn subscribe(&self, channel: Arc<str>, confirmed: watch::Sender<bool>) {
        match self {
            Self::Loopback(_) => {
                confirmed.send_replace(true);
            }
            Self::Valkey(valkey) => valkey.command(Command::Subscribe { channel, confirmed }),
            #[cfg(test)]
            Self::Recording(recording) => recording.subscribe(channel, confirmed),
        }
    }

    pub fn unsubscribe(&self, channel: Arc<str>) {
        match self {
            Self::Loopback(_) => {}
            Self::Valkey(valkey) => valkey.command(Command::Unsubscribe { channel }),
            #[cfg(test)]
            Self::Recording(recording) => recording.unsubscribe(channel),
        }
    }
}
