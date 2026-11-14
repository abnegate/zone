use serde::Serialize;
use uuid::Uuid;

use super::IncludedPull;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preview {
    pub source_id: Uuid,
    pub repository: String,
    pub tag: String,
    pub name: String,
    pub body: String,
    pub base_tag: Option<String>,
    pub target: String,
    pub prerelease: bool,
    pub pulls: Vec<IncludedPull>,
    pub digest: String,
}
