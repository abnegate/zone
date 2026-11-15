use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IncludedPull {
    pub number: u64,
    pub url: String,
    pub title: String,
}
