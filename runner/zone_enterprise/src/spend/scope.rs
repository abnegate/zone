use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Organization,
    Workspace,
    User,
}

impl Scope {
    pub const ALL: [Self; 3] = [Self::Organization, Self::Workspace, Self::User];
}
