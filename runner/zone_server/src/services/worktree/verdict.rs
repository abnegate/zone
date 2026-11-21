//! What the sweep decided about one worktree.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Removable,
    Locked,
    Active,
    NoDefaultBranch,
    Dirty,
    Unmerged,
}

impl Verdict {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Removable => "merged, clean and idle",
            Self::Locked => "locked",
            Self::Active => "used recently",
            Self::NoDefaultBranch => "the repository has no default branch to compare with",
            Self::Dirty => "holds uncommitted changes or untracked files",
            Self::Unmerged => "holds commits the default branch does not",
        }
    }
}
