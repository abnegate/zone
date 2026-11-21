use crate::sso::saml::attribute::Attribute;
use crate::sso::saml::namespace::Namespace;
use crate::sso::saml::node_id::NodeId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub(super) prefix: Option<String>,
    pub(super) local: String,
    pub(super) namespace: Option<String>,
    pub(super) attributes: Vec<Attribute>,
    pub(super) declarations: Vec<Namespace>,
    pub(super) children: Vec<NodeId>,
}

impl Element {
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    pub fn local(&self) -> &str {
        &self.local
    }

    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    pub fn attributes(&self) -> &[Attribute] {
        &self.attributes
    }

    pub fn declarations(&self) -> &[Namespace] {
        &self.declarations
    }

    pub fn children(&self) -> &[NodeId] {
        &self.children
    }

    pub fn is(&self, namespace: &str, local: &str) -> bool {
        self.namespace() == Some(namespace) && self.local == local
    }

    pub fn attribute(&self, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.namespace.is_none() && attribute.local == local)
            .map(Attribute::value)
    }

    pub fn attribute_in(&self, namespace: &str, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.is(namespace, local))
            .map(Attribute::value)
    }
}
