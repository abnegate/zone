use super::merge_method::MergeMethod;

/// What GitHub's GraphQL API says about merging a pull request on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Automation {
    /// Whether the repository lets anyone turn auto-merge on.
    pub allowed: bool,
    pub enabled: bool,
    /// Whether this credential may turn it on or off as it stands.
    pub changeable: bool,
    pub method: Option<MergeMethod>,
}
