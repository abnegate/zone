use std::io;
use std::path::PathBuf;

use zone_core::llm::AgentKind;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The home the session was carried from holds no file for it.
    #[error("The {agent} session {id} has no file to carry")]
    Missing { agent: AgentKind, id: String },
    #[error("Could not carry the session to {}: {source}", path.display())]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}
