use std::path::PathBuf;

use super::Missing;

/// Where a chat's turn starts, as far as can be told without touching the network.
pub enum Start {
    /// The chat has no project, or a source with no local form: it starts where any chat does.
    Elsewhere,
    Directory(PathBuf),
    /// A repository to clone first. Only a turn that is about to run does.
    Missing(Missing),
    /// The project names a place this machine cannot offer, and why, which its chat is told.
    Unavailable(String),
}
