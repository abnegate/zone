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

mod limits;
mod marker;
mod outcome;
mod prompt;
mod provenance;
mod recipe;
mod role;
mod secrets;
mod validation;
mod verdict;

pub use limits::Limits;
pub use marker::{CLOSE_TAG, Marker, MarkerError, OPEN_TAG, VERSION, parse, parse_within};
pub use outcome::VerificationOutcome;
pub use prompt::SYSTEM_PROMPT;
pub use provenance::Provenance;
pub use recipe::Recipe;
pub use role::Role;
pub use verdict::Verdict;
