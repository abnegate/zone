//! Account mail for a test that must not wait on a relay.

use abnegate_notify::Error;
use abnegate_notify::Mail;
use async_trait::async_trait;

/// A relay that accepts the connection and never answers, as a stalled one
/// does until the client's own timeout gives up on it.
pub struct Silent;

#[async_trait]
impl Mail for Silent {
    async fn send(&self, _recipient: &str, _subject: &str, _body: &str) -> Result<(), Error> {
        std::future::pending().await
    }
}

/// A relay that accepts every message at once and keeps who it was for.
#[derive(Default)]
pub struct Recording {
    recipients: std::sync::Mutex<Vec<String>>,
}

impl Recording {
    pub fn recipients(&self) -> Vec<String> {
        self.recipients
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl Mail for Recording {
    async fn send(&self, recipient: &str, _subject: &str, _body: &str) -> Result<(), Error> {
        self.recipients
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(recipient.to_owned());
        Ok(())
    }
}
