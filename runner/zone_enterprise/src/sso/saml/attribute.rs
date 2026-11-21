use std::sync::Arc;

use crate::sso::saml::name::Name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub(super) name: Arc<Name>,
    pub(super) value: Box<str>,
}

impl Attribute {
    pub fn prefix(&self) -> Option<&str> {
        self.name.prefix.as_deref()
    }

    pub fn local(&self) -> &str {
        &self.name.local
    }

    pub fn namespace(&self) -> Option<&str> {
        self.name.namespace.as_deref()
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn is(&self, namespace: &str, local: &str) -> bool {
        self.namespace() == Some(namespace) && self.local() == local
    }
}
