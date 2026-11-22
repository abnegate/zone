use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundleLink {
    pub name: String,
    pub url: String,
}
