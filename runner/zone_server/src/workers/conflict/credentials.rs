//! Where a repair gets the token it presents to the remote.
//!
//! A repair runs until its agent finishes, which can outlast a GitHub App
//! installation token. The token is asked for each time the remote is reached,
//! so the push after a long repair presents one that is still valid rather than
//! the one the checkout was fetched with.

use abnegate_secret::SecretValue;
use async_trait::async_trait;

#[async_trait]
pub trait Credentials: Send + Sync {
    /// The token to present to the remote now, or none for a remote that
    /// needs none.
    async fn token(&self) -> Result<Option<SecretValue>, String>;
}
