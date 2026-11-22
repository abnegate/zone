use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    pub(super) prefix: Option<Arc<str>>,
    pub(super) uri: Arc<str>,
}

impl Namespace {
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }
}
