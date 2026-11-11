//! A chat's optional blocks as its window lets it carry them.

use crate::agent::learned;

/// The three blocks a chat's system entry carries after the capability tail,
/// each ready to append and each empty when there is nothing to show or no
/// room to show it in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fitted {
    /// What the workspace has learned, and which of its entries the text
    /// shows.
    pub findings: learned::Block,
    /// What is remembered about the person.
    pub memory: String,
    /// The workspace's skills index.
    pub skills: String,
}
