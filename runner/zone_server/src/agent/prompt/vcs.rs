//! Version control state of the checkout a turn runs against.

/// Branch and head commit, captured before any tool could change them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vcs {
    pub branch: String,
    pub head: String,
}
