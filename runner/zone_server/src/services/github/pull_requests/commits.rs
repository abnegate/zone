use serde::Serialize;

use super::Commit;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Commits {
    pub number: u64,
    pub head: String,
    pub base: String,
    pub complete: bool,
    pub commits: Vec<Commit>,
}
