use serde_json::Value;

/// One frame the server sent, as the text that arrived and as JSON.
#[derive(Debug, Clone)]
pub struct Received {
    pub text: String,
    pub frame: Value,
}

impl Received {
    pub fn kind(&self) -> &str {
        self.frame["type"].as_str().unwrap_or_default()
    }
}
