//! Behavioral verification the model nominates and the server proves.
//!
//! `receipts` already refuses to build a write record out of model prose. This
//! module extends the same discipline to verification outcomes.
//!
//! The model is given read-only inspection tools and asked to *nominate* a
//! check it believes is relevant. It never runs one and it never grades the
//! result. It says what it found by emitting exactly one strict-JSON marker in
//! its final message, and everything in that marker is validated here: closed
//! enums, byte ceilings, workspace-relative paths only, no traversal, no
//! commands, no prose, no secrets.
//!
//! The outcome the model states is then discarded as authorization. It survives
//! only as [`Provenance::ModelAsserted`] evidence, and a citation carrying that
//! provenance can never pass, exactly as incomplete evidence cannot. Only a
//! server-side execution record mints [`Provenance::ServerExecution`].
//!
//! What turns a nomination into a proof is [`prove`]. It walks the transitive
//! file closure reachable from the nominated entrypoint and digests every file
//! it finds across four trees: both snapshots and both execution roots. Every
//! harness file in that closure must be byte-identical between the predecessor
//! and the target; only the product files — the ones that actually differ — may
//! change. A closure that holds yields a [`ProvenClosure`] witness, and that
//! witness is the only argument [`VerificationOutcome::proven`] accepts.
//!
//! The effect is that a change cannot make its own test pass. A test file the
//! change edited, or added, is a harness file that differs, so the nomination is
//! refused and the refusal names the file.

mod bounds;
mod closure;
mod content;
mod limits;
mod marker;
mod outcome;
mod prompt;
mod proof;
mod provenance;
mod recipe;
mod references;
mod role;
mod roots;
#[cfg(test)]
pub(crate) mod scaffold;
mod secrets;
mod surface;
mod validation;
mod verdict;

pub use bounds::ClosureBounds;
pub use closure::{ClosureError, ClosureProof, Divergence, ProvenClosure, prove};
pub use content::Content;
pub use limits::Limits;
pub use marker::{CLOSE_TAG, Marker, MarkerError, OPEN_TAG, VERSION, parse, parse_within};
pub use outcome::VerificationOutcome;
pub use prompt::SYSTEM_PROMPT;
pub use proof::{FileProof, ProofError};
pub use provenance::Provenance;
pub use recipe::Recipe;
pub use references::{Language, Reference, references};
pub use role::Role;
pub use roots::{Root, RootError, Roots, Tree};
pub use surface::Surface;
pub use verdict::Verdict;
