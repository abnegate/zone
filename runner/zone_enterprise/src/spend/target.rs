use uuid::Uuid;

use crate::spend::error::Error;
use crate::spend::scope::Scope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Target {
    pub scope: Scope,
    pub workspace: Option<Uuid>,
    pub user: Option<Uuid>,
}

impl Target {
    /// Refuses a target that doesn't carry exactly the id its scope needs.
    pub fn new(scope: Scope, workspace: Option<Uuid>, user: Option<Uuid>) -> Result<Self, Error> {
        let shaped = match scope {
            Scope::Organization => workspace.is_none() && user.is_none(),
            Scope::Workspace => workspace.is_some() && user.is_none(),
            Scope::User => workspace.is_none() && user.is_some(),
        };
        if !shaped {
            return Err(Error::ScopeInvalid);
        }
        Ok(Self {
            scope,
            workspace,
            user,
        })
    }

    /// The target a stored budget covers, whose scope follows from which id it carries.
    pub fn of(workspace: Option<Uuid>, user: Option<Uuid>) -> Self {
        let scope = match (workspace, user) {
            (Some(_), _) => Scope::Workspace,
            (None, Some(_)) => Scope::User,
            (None, None) => Scope::Organization,
        };
        Self {
            scope,
            workspace,
            user,
        }
    }
}
