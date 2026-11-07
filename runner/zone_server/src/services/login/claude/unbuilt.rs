//! Why Zone could not build the client a Claude sign-in reaches its token endpoint with.

use abnegate_agent_cli::login::LoginError;

use super::described;

#[derive(Debug, thiserror::Error)]
pub enum Unbuilt {
    #[error("{}", described(.0))]
    Login(#[from] LoginError),
}
