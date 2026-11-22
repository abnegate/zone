use super::refusal::{NAME_INVALID, NAME_RESERVED, Refusal};
use crate::db::organization_members::OrgRole;
use crate::db::workspace_members::WorkspaceRole;

const EMPTY: &str = "A role needs a name";

/// A custom role's name: trimmed, not empty, and never a built-in role's name in any case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Name<'a>(&'a str);

impl<'a> Name<'a> {
    pub fn parse(raw: &'a str) -> Result<Self, Refusal> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(Refusal::Invalid {
                message: EMPTY.to_owned(),
                code: NAME_INVALID,
            });
        }
        let folded = name.to_lowercase();
        if folded.parse::<OrgRole>().is_ok() || folded.parse::<WorkspaceRole>().is_ok() {
            return Err(Refusal::Invalid {
                message: format!("{name} is the name of a built-in role"),
                code: NAME_RESERVED,
            });
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &'a str {
        self.0
    }
}
