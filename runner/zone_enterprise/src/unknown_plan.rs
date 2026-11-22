#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("The plan {0:?} is not one of team or enterprise.")]
pub struct UnknownPlan(pub String);
