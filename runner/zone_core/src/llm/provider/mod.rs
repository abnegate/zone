//! The coding-agent CLI provider.
//!
//! A task can shell out to a coding agent CLI instead of calling a model over
//! HTTP. [`CliProvider`] satisfies
//! [`CompletionProvider`](abnegate_llm::CompletionProvider) like the crate's
//! [`HttpProvider`](abnegate_llm::HttpProvider) does, so a
//! [`Router`](abnegate_llm::Router) can select, split, and fall back between
//! them without the consumer knowing which ran.
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
//! A [`CliProvider`] does not run under [`abnegate_exec`]'s sandbox. That
//! sandbox denies all network access and grants `process-exec` for a single
//! literal command, and a coding agent needs the network to reach its own API
//! and forks a tree of helper processes to do its work. It does reuse that
//! crate's process-group termination, so a timed-out agent takes its whole
//! process tree with it.

mod agent;
mod cli;
mod event;
mod lines;
mod parser;
mod session;
mod settings;
mod transcript;

pub use agent::{AgentKind, Delivery};
pub use cli::{AgentStream, CliProvider, OUTGROWN, STDERR_HEADING};
pub use event::AgentEvent;
pub use lines::{Frame, Lines};
pub use parser::claude::{UNCONFIRMED, UNFUNDED, UNFUNDED_CONTEXT};
pub use session::Session;
pub use settings::{
    BuiltinTools, CliSettings, CodexSandbox, DEFAULT_LINE_LIMIT, DEFAULT_OUTPUT_LIMIT,
    DEFAULT_TIMEOUT, SignIn, Toolset,
};
pub use transcript::{render, render_tail};
