use std::path::PathBuf;
use std::time::Duration;

use abnegate_agent::tool::Session;

/// What a server tool context is built from.
#[derive(Debug, Clone)]
pub struct Config {
    pub working_directory: PathBuf,
    pub session: Session,
    /// Withheld from the file tools, however unrestricted the context.
    pub denied: Vec<PathBuf>,
    /// Whether the tools reach outside `working_directory`.
    pub unrestricted: bool,
    pub maximum_file_size: usize,
    pub command_timeout: Duration,
    /// Whether the chat opted out of the public web, refusing `curl` and
    /// `wget` outright.
    pub chat_offline: bool,
}
