//! The crate's file and command tools as Zone offers them: `run_command`
//! spawning Zone's whole allowlist, and no `wait_for`, which the server
//! registers as its own.

use std::sync::Arc;

use abnegate_agent::ToolRegistry;
use abnegate_agent::tool::DEFAULT_COMMANDS;
use abnegate_agent::tool::RunCommandTool;
use abnegate_agent::tool::job::WAIT_FOR;

mod shell;
#[cfg(test)]
mod tests;

pub use shell::Shell;

/// The programs Zone's `run_command` spawns beyond the crate's
/// [`DEFAULT_COMMANDS`].
pub const EXTRA_COMMANDS: &[&str] = &[
    "node", "deno", "bun", "python", "python3", "ruby", "env", "docker",
];

/// Every program Zone's `run_command` may spawn: [`DEFAULT_COMMANDS`], then
/// [`EXTRA_COMMANDS`].
pub fn allowed() -> Vec<&'static str> {
    DEFAULT_COMMANDS
        .iter()
        .chain(EXTRA_COMMANDS)
        .copied()
        .collect()
}

/// The crate's default tools, with `run_shell` when `shell` includes it,
/// without `wait_for`, and with `run_command` spawning [`allowed`].
pub fn registry(shell: Shell) -> ToolRegistry {
    let mut registry = match shell {
        Shell::Included => ToolRegistry::with_host_tools(),
        Shell::Excluded => ToolRegistry::with_defaults(),
    };
    registry.remove(WAIT_FOR);
    registry.register(Arc::new(RunCommandTool::with_commands(&allowed())));
    registry
}
