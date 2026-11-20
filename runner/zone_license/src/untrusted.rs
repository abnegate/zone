#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Untrusted {
    #[error("The key id {id} is not in the keyring.")]
    UnknownKeyId { id: String },
    #[error("The private key does not match the public key the keyring holds for {id}.")]
    PublicKeyMismatch { id: String },
}
