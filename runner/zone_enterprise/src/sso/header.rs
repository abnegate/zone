use jsonwebtoken::Algorithm;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) algorithm: Algorithm,
    pub(crate) key_id: Option<String>,
}
