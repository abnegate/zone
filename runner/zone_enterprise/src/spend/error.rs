pub const BODY_INVALID: &str = "spend_body_invalid";
pub const RANGE_INVALID: &str = "spend_range_invalid";
pub const LIMIT_INVALID: &str = "budget_limit_invalid";
pub const THRESHOLD_INVALID: &str = "budget_threshold_invalid";
pub const SCOPE_INVALID: &str = "budget_scope_invalid";
pub const BUDGET_EXISTS: &str = "budget_exists";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Error {
    #[error("A budget's limit must be more than $0.")]
    LimitInvalid,
    #[error("A budget's warning threshold must be a whole percentage from 1 to 100.")]
    ThresholdInvalid,
    #[error(
        "A budget covers the organization, one of its workspaces or one of its members. Name only the workspace or member it covers."
    )]
    ScopeInvalid,
    #[error("This organization, workspace or member already has a budget.")]
    Exists,
    #[error("Choose dates as YYYY-MM-DD, with the start on or before the end.")]
    RangeInvalid,
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::LimitInvalid => LIMIT_INVALID,
            Self::ThresholdInvalid => THRESHOLD_INVALID,
            Self::ScopeInvalid => SCOPE_INVALID,
            Self::Exists => BUDGET_EXISTS,
            Self::RangeInvalid => RANGE_INVALID,
        }
    }
}
