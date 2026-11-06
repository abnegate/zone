//! Sealing stored credentials into `ENC[v1:...]` envelopes in place.
//!
//! Every value in a credential column is classified, then sealed, re-sealed,
//! reported or skipped according to what the column's readers accept. Writes
//! are per-row compare-and-swaps committed in batches, behind a verified
//! `pg_dump -Fc` backup of the whole database.

mod ai_key;
mod ai_scope;
mod backup;
mod class;
mod classified;
mod column;
mod error;
mod listing;
mod options;
mod outcome;
mod policy;
mod report;
mod sealer;
mod table;

pub use ai_key::AiKey;
pub use ai_scope::AiScope;
pub use backup::Backup;
pub use class::Class;
pub use classified::Classified;
pub use column::Column;
pub use error::SealingError;
pub use listing::Listing;
pub use options::Options;
pub use outcome::Outcome;
pub use policy::Policy;
pub use report::Report;
pub use sealer::BATCH_SIZE;
pub use sealer::Sealer;
pub use table::Table;
