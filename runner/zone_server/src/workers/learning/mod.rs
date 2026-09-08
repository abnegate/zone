//! What zone learns from the runs it has already finished.
//!
//! One story in four parts. A run produced a change ([`strategy`] records how it went
//! about it, [`observation`] reads what it touched). The change had an outcome
//! ([`attempt`] and [`error_category`] record whether it worked and why not; [`quality`]
//! records how well it was received; [`review`] records what people said about it). And
//! what comes out of that outcome should improve the next run ([`convention`] and
//! [`lesson`] decide what has been confirmed often enough to write down).
//!
//! Every decision is a pure function over in-memory data. [`store`] is the only module
//! that touches the database and [`worker`] is the only one that schedules anything, so
//! each threshold can be exercised without either.
//!
//! The failure mode that matters is a wrong lesson, which silently steers every later
//! run. So each fact has to clear a high bar before it is believed, records what it was
//! derived from and how many separate runs confirmed it, and can be retired with
//! [`crate::db::knowledge::retire_learned_fact`].

pub mod artifacts;
pub mod attempt;
pub mod convention;
pub mod error_category;
pub mod lesson;
pub mod observation;
pub mod quality;
pub mod review;
pub mod store;
pub mod strategy;
pub mod worker;

pub use attempt::{AttemptOutcome, ClassifiedAttempt, OutcomeStatistics, RunAttempt};
pub use convention::{ConventionPolicy, RepoConvention};
pub use error_category::{Categorization, ErrorCategory, ReferenceEmbeddings};
pub use lesson::{LessonPolicy, StrategyLesson, StrategyObservation};
pub use observation::{ChangeType, ConventionKind, ConventionSignal, FileChange};
pub use quality::{ChangeReception, QualityBand, QualityScore, QualityWeights};
pub use review::{ClassifiedComment, ReviewCategory};
pub use strategy::{FixApproach, StrategyFingerprint, ToolInvocation, ToolKind};
pub use worker::{LearningOutcome, LearningPolicy, LearningReport, WorkspaceEvidence};
