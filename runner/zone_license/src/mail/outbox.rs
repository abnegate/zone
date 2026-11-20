use async_trait::async_trait;

use crate::mail::{Letter, MailError};

#[async_trait]
pub trait Outbox: Send + Sync {
    async fn send(&self, recipient: &str, letter: &Letter) -> Result<(), MailError>;
}
