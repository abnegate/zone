mod attribute;
mod builder;
pub mod canonical;
mod document;
mod element;
mod malformed;
mod namespace;
mod node;
mod node_id;
mod refusal;
mod scope;
#[cfg(any(test, feature = "signing"))]
pub mod testing;
#[cfg(test)]
mod tests;
pub mod uri;

pub use attribute::Attribute;
pub use document::Document;
pub use element::Element;
pub use malformed::Malformed;
pub use namespace::Namespace;
pub use node::Node;
pub use node_id::NodeId;
pub use refusal::Refusal;
