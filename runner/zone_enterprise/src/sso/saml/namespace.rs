#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    pub(super) prefix: Option<String>,
    pub(super) uri: String,
}

impl Namespace {
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }
}
