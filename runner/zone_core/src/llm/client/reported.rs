use std::sync::{Arc, Mutex, PoisonError};

use abnegate_agent_cli::{AgentKind, Occupancy};

/// Who said which model answered: the coding agent that wrote the turn, or
/// the endpoint that answered it. A report is only this client's while it
/// still runs on whoever made it, so a turn handed from one agent to another,
/// or from an endpoint to an agent, never reads the other's as its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reporter {
    Agent(AgentKind),
    Endpoint,
}

/// What a backend last reported of the turn it ran -- its context and the model it ran on -- and
/// who reported it, shared by every clone of the client that ran it.
#[derive(Debug, Clone, Default)]
pub(super) struct Reported(Arc<Mutex<Latest>>);

/// The latest report of each kind, each kept with whoever made it.
#[derive(Debug, Clone, Default)]
struct Latest {
    occupancy: Option<(AgentKind, Occupancy)>,
    model: Option<(Reporter, String)>,
}

impl Reported {
    pub(super) fn record(&self, agent: AgentKind, occupancy: Occupancy) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .occupancy = Some((agent, occupancy));
    }

    pub(super) fn latest(&self) -> Option<(AgentKind, Occupancy)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .occupancy
    }

    /// Keep `model` as what `reporter` says answered, unless it is blank:
    /// a server that names no model has reported nothing.
    pub(super) fn record_model(&self, reporter: Reporter, model: &str) {
        let model = model.trim();
        if model.is_empty() {
            return;
        }
        self.0.lock().unwrap_or_else(PoisonError::into_inner).model =
            Some((reporter, model.to_string()));
    }

    pub(super) fn latest_model(&self) -> Option<(Reporter, String)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .model
            .clone()
    }
}
