use serde::{Deserialize, Deserializer, Serialize};

use super::closure::ProvenClosure;
use super::marker::Marker;
use super::provenance::Provenance;
use super::recipe::Recipe;
use super::verdict::Verdict;

/// A verification verdict together with how it was produced.
///
/// [`Self::asserted`] wraps what a model claimed and [`Self::proven`] wraps what
/// a server-side execution recorded over a harness closure that held. Those two
/// constructors are the only way to build one, the fields are private so no
/// struct literal can forge a third, and only the second can carry a passing
/// result.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct VerificationOutcome {
    verdict: Verdict,
    provenance: Provenance,
    recipes: Vec<Recipe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    closure: Option<String>,
}

impl VerificationOutcome {
    /// What the model claimed, from its parsed marker. Advisory evidence only.
    pub fn asserted(marker: Marker) -> Self {
        Self {
            verdict: marker.outcome,
            provenance: Provenance::ModelAsserted,
            recipes: marker.recipes,
            closure: None,
        }
    }

    /// What a server-side execution proved over a harness closure.
    ///
    /// The witness is only obtainable from a [`ClosureProof`] that held, so an
    /// authoritative outcome cannot exist without one: a change that edited the
    /// harness it nominated has no witness to pass here.
    ///
    /// [`ClosureProof`]: super::ClosureProof
    pub fn proven(closure: ProvenClosure<'_>, verdict: Verdict, recipes: Vec<Recipe>) -> Self {
        Self {
            verdict,
            provenance: Provenance::ServerExecution,
            recipes,
            closure: Some(closure.proof().entrypoint().to_string()),
        }
    }

    pub const fn verdict(&self) -> Verdict {
        self.verdict
    }

    /// The entrypoint of the closure that proved this outcome, if one did.
    pub fn closure(&self) -> Option<&str> {
        self.closure.as_deref()
    }

    pub const fn provenance(&self) -> Provenance {
        self.provenance
    }

    pub fn recipes(&self) -> &[Recipe] {
        &self.recipes
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
    /// over a closure that held.
    pub const fn passed(&self) -> bool {
        self.provenance.authoritative() && matches!(self.verdict, Verdict::Verified)
    }
}

#[derive(Deserialize)]
struct Wire {
    verdict: Verdict,
    recipes: Vec<Recipe>,
}

/// An outcome that arrives as data is advisory, whatever it says about itself.
///
/// Serialized bytes are not an execution record, and nothing that crosses a
/// deserialization boundary carries the closure witness that proved it, so the
/// provenance in the payload is discarded rather than trusted.
impl<'de> Deserialize<'de> for VerificationOutcome {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            verdict: wire.verdict,
            provenance: Provenance::ModelAsserted,
            recipes: wire.recipes,
            closure: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::scaffold::{Workspace, holding};
    use super::super::{ClosureProof, parse};
    use super::*;

    const ENTRYPOINT: &str = "tests/checkout.test.mjs";

    #[test]
    fn a_closure_that_held_is_the_only_route_to_a_passing_outcome() {
        let proof = holding().prove(ENTRYPOINT).expect("the closure proves");
        let witness = proof.witness().expect("a closure that held has a witness");

        let outcome = VerificationOutcome::proven(witness, Verdict::Verified, Vec::new());

        assert!(outcome.passed());
        assert!(outcome.authoritative());
        assert_eq!(outcome.provenance(), Provenance::ServerExecution);
        assert_eq!(outcome.closure(), Some(ENTRYPOINT));
    }

    #[test]
    fn a_refused_closure_yields_no_witness_to_pass_to_proven() {
        let proof = refused();

        assert!(!proof.identical());
        assert!(
            proof.witness().is_none(),
            "a refused closure cannot mint an authoritative outcome"
        );

        let outcome = match proof.witness() {
            Some(witness) => VerificationOutcome::proven(witness, Verdict::Verified, Vec::new()),
            None => VerificationOutcome::asserted(marker()),
        };

        assert!(!outcome.passed());
        assert!(outcome.advisory());
        assert_eq!(outcome.closure(), None);
    }

    #[test]
    fn an_asserted_outcome_never_passes_however_confident_the_model_is() {
        let outcome = VerificationOutcome::asserted(marker());

        assert_eq!(outcome.verdict(), Verdict::Verified);
        assert!(outcome.complete());
        assert!(outcome.advisory());
        assert!(!outcome.passed());
        assert!(!outcome.authoritative());
    }

    #[test]
    fn an_outcome_that_arrives_as_data_is_advisory_however_it_was_serialized() {
        let proof = holding().prove(ENTRYPOINT).expect("the closure proves");
        let witness = proof.witness().expect("a closure that held has a witness");
        let proven = VerificationOutcome::proven(witness, Verdict::Verified, Vec::new());

        let payload = serde_json::to_string(&proven).expect("the outcome serializes");
        let restored: VerificationOutcome =
            serde_json::from_str(&payload).expect("the outcome deserializes");

        assert!(proven.passed());
        assert!(!restored.passed(), "a proof does not survive serialization");
        assert!(restored.advisory());
        assert_eq!(restored.verdict(), Verdict::Verified);
        assert_eq!(restored.closure(), None);
    }

    #[test]
    fn a_forged_provenance_in_a_payload_is_discarded() {
        let restored: VerificationOutcome = serde_json::from_str(
            r#"{"verdict":"verified","provenance":"server_execution","recipes":[],"closure":"tests/checkout.test.mjs"}"#,
        )
        .expect("the payload deserializes");

        assert!(!restored.passed());
        assert_eq!(restored.provenance(), Provenance::ModelAsserted);
        assert_eq!(restored.closure(), None);
    }

    fn refused() -> ClosureProof {
        let workspace = Workspace::new();
        workspace.unchanged(ENTRYPOINT, "import './helper.mjs';\n");
        workspace.predecessor("tests/helper.mjs", "export const seed = () => 0;\n");
        workspace.target("tests/helper.mjs", "export const seed = () => 1;\n");
        workspace
            .prove(ENTRYPOINT)
            .expect("the closure is evaluated")
    }

    fn marker() -> Marker {
        parse(concat!(
            "<zone-verification>",
            r#"{"version":1,"outcome":"verified","recipes":[]}"#,
            "</zone-verification>"
        ))
        .expect("the fixture marker parses")
    }
}
