use crate::sso::saml::builder::Builder;
use crate::sso::saml::element::Element;
use crate::sso::saml::malformed::Malformed;
use crate::sso::saml::node::Node;
use crate::sso::saml::node_id::NodeId;

const BYTE_ORDER_MARK: &str = "\u{feff}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub(super) nodes: Vec<Node>,
    pub(super) parents: Vec<Option<NodeId>>,
    pub(super) root: NodeId,
}

impl Document {
    pub fn parse(input: &[u8]) -> Result<Self, Malformed> {
        let text = std::str::from_utf8(input).map_err(|_| Malformed::Encoding)?;
        let text = text.strip_prefix(BYTE_ORDER_MARK).unwrap_or(text);
        Builder::new().build(text)
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0)
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match self.node(id) {
            Some(Node::Element(element)) => Some(element),
            _ => None,
        }
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.parents.get(id.0).copied().flatten()
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.element(id).map_or(&[], Element::children)
    }

    pub fn child_elements(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(id)
            .iter()
            .copied()
            .filter(|child| self.element(*child).is_some())
    }

    pub fn children_named<'a>(
        &'a self,
        id: NodeId,
        namespace: &'a str,
        local: &'a str,
    ) -> impl Iterator<Item = NodeId> + 'a {
        self.children(id)
            .iter()
            .copied()
            .filter(move |child| self.is(*child, namespace, local))
    }

    pub fn descendants(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut stack = Vec::new();
        if self.node(id).is_some() {
            stack.push(id);
        }
        std::iter::from_fn(move || {
            let next = stack.pop()?;
            stack.extend(self.children(next).iter().rev().copied());
            Some(next)
        })
    }

    pub fn is(&self, id: NodeId, namespace: &str, local: &str) -> bool {
        self.element(id)
            .is_some_and(|element| element.is(namespace, local))
    }

    pub fn text(&self, id: NodeId) -> Option<String> {
        let mut text = String::new();
        for child in self.element(id)?.children() {
            match self.node(*child)? {
                Node::Element(_) => return None,
                Node::Text(value) => text.push_str(value),
                Node::Comment | Node::Instruction { .. } => {}
            }
        }
        Some(text)
    }
}
