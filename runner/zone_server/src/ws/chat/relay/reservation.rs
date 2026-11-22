use dashmap::DashMap;
use once_cell::sync::Lazy;
use uuid::Uuid;

use crate::services::chat::session::Session;

/// Leases taken for a forwarded message before it was queued here, each
/// waiting for the turn that runs the message to take it over.
static RESERVED: Lazy<DashMap<Uuid, Session>> = Lazy::new(DashMap::new);

/// A chat's reserved lease, claimed by the turn that runs under it. Whatever
/// the turn does not take is released when this drops.
pub struct Reservation(Option<Session>);

impl Reservation {
    pub fn hold(chat: Uuid, session: Session) {
        if let Some(earlier) = RESERVED.insert(chat, session) {
            drop(Self(Some(earlier)));
        }
    }

    pub fn claim(chat: Uuid) -> Self {
        Self(RESERVED.remove(&chat).map(|(_, session)| session))
    }

    pub fn take(&mut self) -> Option<Session> {
        self.0.take()
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Some(mut session) = self.0.take() {
            tokio::spawn(async move {
                if let Err(error) = session.close().await {
                    tracing::warn!(%error, "Could not release a chat lease nothing ran under");
                }
            });
        }
    }
}
