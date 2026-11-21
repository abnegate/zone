#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub(super) prefix: Option<String>,
    pub(super) local: String,
    pub(super) namespace: Option<String>,
    pub(super) value: String,
}

impl Attribute {
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    pub fn local(&self) -> &str {
        &self.local
    }

    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn is(&self, namespace: &str, local: &str) -> bool {
        self.namespace() == Some(namespace) && self.local == local
    }
}
