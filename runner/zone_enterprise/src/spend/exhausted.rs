use crate::spend::scope::Scope;

pub const BUDGET_EXHAUSTED: &str = "budget_exhausted";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "{} monthly AI budget is used up, so this request wasn't sent. An organization admin can raise it under Settings › Spend; it resets on the 1st (UTC).",
    owner(.scope)
)]
pub struct Exhausted {
    pub scope: Scope,
}

impl Exhausted {
    pub fn code(&self) -> &'static str {
        BUDGET_EXHAUSTED
    }
}

fn owner(scope: &Scope) -> &'static str {
    match scope {
        Scope::Organization => "Your organization's",
        Scope::Workspace => "This workspace's",
        Scope::User => "Your",
    }
}
