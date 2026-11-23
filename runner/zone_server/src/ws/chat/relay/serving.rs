use std::time::Duration;

use once_cell::sync::Lazy;
use redis::aio::ConnectionManager;
use redis::{Script, SetExpiry, SetOptions};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cluster::{Bus, Namespace};

const PREFIX: &str = "zone";
const CHAT: &str = "chat";
const SERVING: &str = "serving";

/// How long Valkey may take to read or write a chat's serving key.
const TIMEOUT: Duration = Duration::from_millis(500);

static ERASE: Lazy<Script> = Lazy::new(|| {
    Script::new(
        "if redis.call('GET', KEYS[1]) == ARGV[1] then return redis.call('DEL', KEYS[1]) else return 0 end",
    )
});

/// The replica answering a chat's control topic, as Valkey has it under the
/// chat's serving key. `holding` says whether it runs the chat's turn or keeps
/// messages queued for one, rather than only waiting out its grace period.
/// `answerer` tells apart the keys two answering tasks of one replica wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Serving {
    pub instance: Uuid,
    pub answerer: Uuid,
    pub holding: bool,
}

impl Serving {
    pub fn key(namespace: &Namespace, chat: Uuid) -> String {
        format!("{PREFIX}:{namespace}:{CHAT}:{chat}:{SERVING}")
    }

    /// The replica other than this one answering `chat`. None when nobody
    /// does, this replica does, or Valkey cannot say, so the caller falls
    /// back on the chat's lease.
    pub async fn elsewhere(bus: &Bus, chat: Uuid) -> Option<Self> {
        let mut connection = connection(bus)?;
        let key = Self::key(bus.namespace(), chat);
        let mut command = redis::cmd("GET");
        command.arg(&key);
        let read = command.query_async::<Option<String>>(&mut connection);
        let payload = match tokio::time::timeout(TIMEOUT, read).await {
            Ok(Ok(payload)) => payload?,
            Ok(Err(error)) => {
                tracing::warn!(%chat, %error, "Could not read which replica serves the chat");
                return None;
            }
            Err(_) => {
                tracing::warn!(%chat, "Valkey did not say in time which replica serves the chat");
                return None;
            }
        };
        let serving: Self = serde_json::from_str(&payload)
            .inspect_err(|error| tracing::warn!(%chat, %error, "Dropped an unreadable serving key"))
            .ok()?;
        (serving.instance != bus.instance()).then_some(serving)
    }

    /// Writes this as `chat`'s serving key for `lifetime`, and returns what
    /// was written, which [`Self::erase`] takes to remove only this write.
    pub async fn write(&self, bus: &Bus, chat: Uuid, lifetime: Duration) -> Option<String> {
        let mut connection = connection(bus)?;
        let payload = serde_json::to_string(self)
            .inspect_err(
                |error| tracing::error!(%chat, %error, "Could not serialize a serving key"),
            )
            .ok()?;
        let milliseconds = u64::try_from(lifetime.as_millis()).unwrap_or(u64::MAX);
        let options = SetOptions::default().with_expiration(SetExpiry::PX(milliseconds));
        let mut command = redis::cmd("SET");
        command
            .arg(Self::key(bus.namespace(), chat))
            .arg(&payload)
            .arg(options);
        let written = command.query_async::<()>(&mut connection);
        match tokio::time::timeout(TIMEOUT, written).await {
            Ok(Ok(())) => Some(payload),
            Ok(Err(error)) => {
                tracing::warn!(%chat, %error, "Could not say this replica serves the chat");
                None
            }
            Err(_) => {
                tracing::warn!(%chat, "Valkey did not take in time that this replica serves the chat");
                None
            }
        }
    }

    /// Removes `chat`'s serving key if it still holds `payload`, leaving one
    /// another answering task wrote since.
    pub async fn erase(bus: &Bus, chat: Uuid, payload: &str) {
        let Some(mut connection) = connection(bus) else {
            return;
        };
        let key = Self::key(bus.namespace(), chat);
        let mut invocation = ERASE.key(&key);
        invocation.arg(payload);
        let erased = invocation.invoke_async::<i64>(&mut connection);
        match tokio::time::timeout(TIMEOUT, erased).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                tracing::warn!(%chat, %error, "Could not remove the chat's serving key");
            }
            Err(_) => tracing::warn!(%chat, "Valkey did not remove the chat's serving key in time"),
        }
    }
}

fn connection(bus: &Bus) -> Option<ConnectionManager> {
    bus.valkey().filter(|_| bus.connected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;

    const PASSPHRASE: &str = "a-serving-passphrase-of-at-least-32-bytes";

    #[test]
    fn the_serving_key_sits_beside_the_chat_topics_in_the_namespace() {
        let namespace = Namespace::derive(&secret::derive(PASSPHRASE).unwrap());
        let chat = Uuid::new_v4();

        assert_eq!(
            Serving::key(&namespace, chat),
            format!("zone:{namespace}:chat:{chat}:serving")
        );
    }

    #[test]
    fn a_serving_key_reads_back_as_written() {
        let serving = Serving {
            instance: Uuid::new_v4(),
            answerer: Uuid::new_v4(),
            holding: true,
        };

        let payload = serde_json::to_string(&serving).unwrap();

        assert_eq!(serde_json::from_str::<Serving>(&payload).unwrap(), serving);
    }
}
