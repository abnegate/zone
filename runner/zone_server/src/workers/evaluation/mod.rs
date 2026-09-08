//! Code quality evaluation for task runs.
//!
//! A run's exit code says the agent's last command succeeded, not that the
//! change is any good. This module measures the workspace before the agent
//! works and again afterwards, and reports the difference: tests passing and
//! failing, lint findings, typecheck findings, and coverage.
//!
//! Detection and delta computation are pure over a directory tree and captured
//! output. Execution goes through the tool runner's executor, which puts each
//! tool in its own process group behind a timeout and an output cap.

pub mod category;
pub mod delta;
pub mod detector;
pub mod diagnostic;
pub mod evaluator;
pub mod execution;
pub mod lookup;
pub mod parser;
pub mod settings;
pub mod snapshot;

pub use category::EvalCategory;
pub use delta::{EvalDelta, EvaluationReport, Verdict};
pub use detector::{DetectedTool, Ecosystem, OutputFormat, detect};
pub use diagnostic::{Diagnostic, DiagnosticSeverity};
pub use evaluator::Evaluator;
pub use settings::{CategorySelection, EvaluationSettings};
pub use snapshot::{EvalSnapshot, RunOutcome};
