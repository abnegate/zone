/// A way GitHub can merge a pull request, as its GraphQL API names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Squash,
    Merge,
    Rebase,
}

impl MergeMethod {
    /// The order a method is picked in when a repository allows several.
    pub const PREFERRED: [Self; 3] = [Self::Squash, Self::Merge, Self::Rebase];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Squash => "SQUASH",
            Self::Merge => "MERGE",
            Self::Rebase => "REBASE",
        }
    }
}
