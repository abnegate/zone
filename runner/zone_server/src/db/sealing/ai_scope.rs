/// Which AI settings table a key column belongs to.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AiScope {
    Organization,
    Workspace,
}

impl AiScope {
    /// The table the scope's settings are stored in.
    pub fn table(self) -> &'static str {
        match self {
            Self::Organization => "organization_ai_settings",
            Self::Workspace => "workspace_ai_settings",
        }
    }
}
