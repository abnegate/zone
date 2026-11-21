use crate::sso::saml::node_id::NodeId;

pub(super) struct Frame {
    pub(super) id: NodeId,
    pub(super) mark: usize,
    pub(super) children: Vec<NodeId>,
}
