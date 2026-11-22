use crate::sso::saml::element::Element;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
    Comment,
    Instruction { target: String, data: String },
}
