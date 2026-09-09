use serde::{Deserialize, Serialize};

/// How an outcome was produced.
///
/// A server-side execution record proves an outcome. A model only claims one,
/// and a claim is advisory evidence that never authorizes a passing result.
#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    #[default]
    ServerExecution,
    ModelAsserted,
}

impl Provenance {
    pub const fn authoritative(self) -> bool {
        matches!(self, Self::ServerExecution)
    }

    pub const fn advisory(self) -> bool {
        matches!(self, Self::ModelAsserted)
    }
}
