#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Malformed {
    #[error("it isn't encoded as UTF-8")]
    Encoding,
    #[error("it isn't XML 1.0")]
    Version,
    #[error("it declares a document type")]
    Doctype,
    #[error("it refers to an entity other than the five predefined XML entities")]
    Entity,
    #[error("it contains a character XML doesn't allow")]
    Character,
    #[error("it contains a name that isn't a valid XML name")]
    Name,
    #[error("it uses a namespace prefix that isn't declared")]
    Prefix,
    #[error("it declares a namespace XML doesn't allow")]
    Namespace,
    #[error("an element repeats an attribute")]
    DuplicateAttribute,
    #[error("it doesn't have exactly one root element")]
    Root,
    #[error("it isn't well-formed XML")]
    Syntax,
}
