use std::sync::Arc;

use redis::aio::ConnectionManager;
use tokio::sync::watch;

use super::command::Command;
use super::loopback::Loopback;
use super::packet::Packet;
use super::valkey::Valkey;

/// How envelopes leave this process and reach the others.
#[derive(Debug)]
pub enum Transport {
    Loopback(Loopback),
    Valkey(Valkey),
}

impl Transport {
    /// Whether an envelope published now could reach another instance, so a
    /// lone loopback bus skips sealing envelopes nobody would hear.
    pub fn reaches_peers(&self) -> bool {
        match self {
            Self::Loopback(loopback) => loopback.has_peers(),
            Self::Valkey(_) => true,
        }
    }

    pub fn valkey(&self) -> Option<&ConnectionManager> {
        match self {
            Self::Loopback(_) => None,
            Self::Valkey(valkey) => Some(valkey.manager()),
        }
    }

    pub fn send(&self, packet: Packet) {
        match self {
            Self::Loopback(loopback) => loopback.send(packet),
            Self::Valkey(valkey) => valkey.send(packet),
        }
    }

    pub fn subscribe(&self, channel: Arc<str>, confirmed: watch::Sender<bool>) {
        match self {
            Self::Loopback(_) => {
                confirmed.send_replace(true);
            }
            Self::Valkey(valkey) => valkey.command(Command::Subscribe { channel, confirmed }),
        }
    }

    pub fn unsubscribe(&self, channel: Arc<str>) {
        if let Self::Valkey(valkey) = self {
            valkey.command(Command::Unsubscribe { channel });
        }
    }
}
