use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Merged {
    pub number: u64,
    pub sha: Option<String>,
    pub admin: bool,
    pub url: String,
}
