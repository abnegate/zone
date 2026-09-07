use serde::{Deserialize, Serialize};

use super::marker::Marker;
use super::provenance::Provenance;
use super::recipe::Recipe;
use super::verdict::Verdict;

/// A verification verdict together with how it was produced.
///
/// [`Self::asserted`] wraps what a model claimed and [`Self::proven`] wraps what
/// a server-side execution recorded. Those two constructors are the only way to
/// build one, so provenance can never be forgotten, and only the second can
/// carry a passing result.
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerificationOutcome {
    pub verdict: Verdict,
    pub provenance: Provenance,
    pub recipes: Vec<Recipe>,
}

impl VerificationOutcome {
    /// What the model claimed, from its parsed marker. Advisory evidence only.
    pub fn asserted(marker: Marker) -> Self {
        Self {
            verdict: marker.outcome,
            provenance: Provenance::ModelAsserted,
            recipes: marker.recipes,
        }
    }

    /// What a server-side execution record proved.
    pub fn proven(verdict: Verdict, recipes: Vec<Recipe>) -> Self {
        Self {
            verdict,
            provenance: Provenance::ServerExecution,
            recipes,
        }
    }

    pub const fn authoritative(&self) -> bool {
        self.provenance.authoritative()
    }

    pub const fn advisory(&self) -> bool {
        self.provenance.advisory()
    }

    /// Whether the check reached a determination at all. An unavailable check
    /// leaves the evidence incomplete rather than negative.
    pub const fn complete(&self) -> bool {
        self.verdict.determined()
    }

    /// The only route to a passing behavioral result: a server-side execution
    /// that verified the behavior.
    pub const fn passed(&self) -> bool {
        self.provenance.authoritative() && matches!(self.verdict, Verdict::Verified)
    }
}
