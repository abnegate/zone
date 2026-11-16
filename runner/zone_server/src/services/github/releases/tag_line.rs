use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagLine {
    pub prefix: Option<String>,
    pub latest_tag: Option<String>,
    pub next_tag: String,
    pub previous_tags: Vec<String>,
}
