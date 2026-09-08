//! Blast radius, severity scoring and suppression.
//!
//! One judgement shared by two callers. The task queue asks which of the work
//! it is holding should run next; a pull request asks how far the change in
//! front of it reaches. Both reduce to the same question — how much does this
//! matter — so both answer it with the same vocabulary, the same weights and
//! the same suppression rules, all of which are configuration rather than
//! constants.
//!
//! A change is scored from five signals: declared severity, how far it reaches,
//! how much it looks like a regression, its blast radius, and whether it
//! belongs to a cluster of related work. Where a signal is missing the module
//! assumes the conservative answer rather than the convenient one.

pub mod blast_radius;
pub mod change;
pub mod configuration;
pub mod patterns;
pub mod prioritiser;
pub mod pull_request;
pub mod queue;
pub mod ratio;
pub mod score;
pub mod signals;
pub mod suppression;
pub mod weights;

pub use blast_radius::BlastRadius;
pub use change::{Change, Origin};
pub use configuration::Configuration;
pub use patterns::PathPatterns;
pub use prioritiser::{Prioritiser, Verdict};
pub use pull_request::RiskSignal;
pub use queue::{Queue, QueuedTask};
pub use ratio::Ratio;
pub use score::Score;
pub use signals::{Level, Severity, Signals};
pub use suppression::{Outcome, Rule, RuleSet};
pub use weights::Weights;
