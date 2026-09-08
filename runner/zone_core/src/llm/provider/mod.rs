//! Completion providers.
//!
//! A task runs on a local model through LiteLLM or shells out to a coding
//! agent CLI, and nothing above this module needs to know which. Both satisfy
//! [`CompletionProvider`]; [`Router`] satisfies it too, over a set of them, so
//! selection, A/B splitting, and fallback are invisible to the consumer.
//!
//! # Failure classification
//!
//! This module does not decide whether a failure is worth retrying. The task
//! worker already owns that judgement and makes it by reading the failure
//! text, so a provider's job is to report what went wrong in the words the
//! underlying service used, with any credential scrubbed out.
//!
//! # Confinement
//!
//! A [`CliProvider`] does not run under [`tool_runner`]'s sandbox. That
//! sandbox denies all network access and grants `process-exec` for a single
//! literal command, and a coding agent needs the network to reach its own API
//! and forks a tree of helper processes to do its work. It does reuse that
//! crate's process-group termination and output caps, so a timed-out agent
//! takes its whole process tree with it.

mod agent;
mod cli;
mod completion;
mod credential;
mod error;
mod event;
mod http;
mod lines;
mod parser;
mod router;
mod selection;
mod settings;
mod transcript;

#[cfg(test)]
mod testing;

pub use agent::{AgentKind, Delivery};
pub use cli::CliProvider;
pub use completion::{Completion, CompletionProvider, CompletionRequest, ProviderKind};
pub use credential::Credential;
pub use error::{ExitStatus, ProviderError};
pub use event::AgentEvent;
pub use http::HttpProvider;
pub use lines::{Lines, Overlong};
pub use router::Router;
pub use selection::{SelectionStrategy, Weighted, choose, sample};
pub use settings::{CliSettings, DEFAULT_LINE_LIMIT, DEFAULT_OUTPUT_LIMIT, DEFAULT_TIMEOUT};
pub use transcript::render;
