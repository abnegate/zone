use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Name {
    pub(super) prefix: Option<Arc<str>>,
    pub(super) local: Arc<str>,
    pub(super) namespace: Option<Arc<str>>,
}
