use super::CheckDetail;
use crate::services::github::readiness::Assessment;
use crate::services::github::readiness::ReviewComment;
use crate::services::prioritisation::RiskSignal;

#[derive(Debug, Clone)]
pub struct Assessed {
    pub assessment: Assessment,
    pub risk: RiskSignal,
    pub checks: Vec<CheckDetail>,
    pub comments: Vec<ReviewComment>,
}
