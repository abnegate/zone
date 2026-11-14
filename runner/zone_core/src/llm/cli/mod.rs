//! Zone's side of running a coding agent CLI through [`abnegate_agent_cli`].
//!
//! The crate drives the agent. What stays here is Zone's own domain: the
//! tools Zone serves a turn over MCP, whether the agent keeps its own, whose
//! sign-in it runs under, who watches what its tools did, and the
//! environment every child Zone starts is confined to.

use std::time::Duration;

mod builtin_tools;
mod confinement;
mod inherited;
mod observer;
mod passthrough;
#[cfg(any(test, feature = "test-support"))]
mod recorded;
mod settings;
mod sign_in;
mod toolset;

pub use builtin_tools::BuiltinTools;
pub use confinement::{BASE_URLS, confined};
pub use inherited::{EXTRAS, inheritable, inherited};
pub use observer::Observer;
pub use passthrough::{PASSTHROUGH, passthrough};
#[cfg(any(test, feature = "test-support"))]
pub use recorded::prompt;
pub use settings::settings;
pub use sign_in::SignIn;
pub use toolset::Toolset;

/// How long an agent waits on one call to Zone's tools, an approval the call
/// is held for included.
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How long Codex waits for Zone's MCP server to start.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
