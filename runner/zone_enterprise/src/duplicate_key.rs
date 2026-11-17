#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("The keyring lists the key {id} more than once.")]
pub struct DuplicateKey {
    pub id: String,
}
