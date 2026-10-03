use zone_core::llm::AgentKind;

use crate::db::agent_logins::AgentLoginRow;
use crate::services::login::credential::Login;
use crate::services::login::usage::Snapshot;

/// The login a session runs under, resolved for its turns, with the usage it was chosen on.
#[derive(Debug)]
pub struct Chosen {
    pub login: AgentLoginRow,
    pub agent: AgentKind,
    pub resolved: Login,
    pub snapshot: Option<Snapshot>,
}
