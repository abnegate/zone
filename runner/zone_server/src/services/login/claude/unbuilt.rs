//! Why Zone could not build the client a Claude sign-in reaches its token endpoint with.

use abnegate_agent_cli::login::LoginError;

use super::PROXY_REFUSED;
use super::described;

#[derive(Debug, thiserror::Error)]
pub enum Unbuilt {
    /// The server's proxy for the token endpoint names a username or a password, which the
    /// sign-in client refuses to send.
    #[error("{PROXY_REFUSED}")]
    CredentialedProxy,
    #[error("{}", described(.0))]
    Login(#[from] LoginError),
}
