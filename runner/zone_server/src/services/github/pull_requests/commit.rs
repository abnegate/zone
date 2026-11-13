use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Commit {
    pub sha: String,
    pub message: String,
    pub author_name: Option<String>,
    pub author_login: Option<String>,
    pub authored_at: Option<String>,
    pub url: String,
}
