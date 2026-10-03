use zone_core::llm::AgentKind;

use super::error::Error;
use crate::db::agent_logins::AgentLoginRow;
use crate::services::login::credential;

/// Why the logins that could not be resolved were left out: the first of the preferred agent's,
/// else the first of any.
pub(super) struct Failure {
    preferred: AgentKind,
    first: Option<(AgentKind, credential::Error)>,
}

impl Failure {
    pub(super) fn new(preferred: AgentKind) -> Self {
        Self {
            preferred,
            first: None,
        }
    }

    /// Leaves `login` out for `error`, unless the store itself failed. A login signed out
    /// meanwhile is gone, and says nothing about the others.
    pub(super) fn record(
        &mut self,
        agent: AgentKind,
        login: &AgentLoginRow,
        error: credential::Error,
    ) -> Result<(), Error> {
        match error {
            credential::Error::Database(source) => Err(Error::Database(source)),
            credential::Error::Deleted => Ok(()),
            error => {
                tracing::warn!(
                    organization = %login.organization_id,
                    login = %login.id,
                    %agent,
                    %error,
                    "A login cannot be used; routing around it"
                );
                let replaces = match &self.first {
                    None => true,
                    Some((first, _)) => *first != self.preferred && agent == self.preferred,
                };
                if replaces {
                    self.first = Some((agent, error));
                }
                Ok(())
            }
        }
    }

    pub(super) fn into_error(self) -> Error {
        match self.first {
            Some((agent, source)) => Error::Unresolved { agent, source },
            None => Error::None,
        }
    }
}
