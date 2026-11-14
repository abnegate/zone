use serde::Serialize;

use super::CheckKind;
use crate::services::github::readiness::CheckState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckDetail {
    pub kind: CheckKind,
    pub id: Option<u64>,
    pub name: String,
    pub state: CheckState,
    pub url: Option<String>,
    pub updated_at: Option<String>,
}
