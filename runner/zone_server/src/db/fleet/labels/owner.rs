#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Owner {
    Organization,
    Workspace,
}

impl Owner {
    pub(super) fn table(self) -> &'static str {
        match self {
            Self::Organization => "organization_runner_labels",
            Self::Workspace => "workspace_runner_labels",
        }
    }

    pub(super) fn column(self) -> &'static str {
        match self {
            Self::Organization => "organization_id",
            Self::Workspace => "workspace_id",
        }
    }
}
