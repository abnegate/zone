use std::sync::{Arc, Mutex, PoisonError};

use abnegate_agent_cli::{AgentKind, Occupancy};

/// What a coding agent last reported of its context, and which agent did, shared by every clone
/// of the client that ran it.
#[derive(Debug, Clone, Default)]
pub(super) struct Reported(Arc<Mutex<Option<(AgentKind, Occupancy)>>>);

impl Reported {
    pub(super) fn record(&self, agent: AgentKind, occupancy: Occupancy) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some((agent, occupancy));
    }

    pub(super) fn latest(&self) -> Option<(AgentKind, Occupancy)> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
