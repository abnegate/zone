/// The URIs that start with `prefix` and end with `suffix`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UriNamespace {
    pub prefix: String,
    pub suffix: String,
}

impl UriNamespace {
    pub fn new(prefix: impl Into<String>, suffix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            suffix: suffix.into(),
        }
    }
}
