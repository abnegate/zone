#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Rejection {
    #[error("it isn't a well-formed ID token")]
    Malformed,
    #[error("its signing algorithm isn't allowed")]
    Algorithm,
    #[error("it was signed with a key the identity provider doesn't publish")]
    UnknownKey,
    #[error("its signing key doesn't match its algorithm")]
    KeyMismatch,
    #[error("its signature is invalid")]
    Signature,
    #[error("it was issued by a different issuer")]
    Issuer,
    #[error("it was issued to a different client")]
    Audience,
    #[error("its authorized party isn't this client")]
    AuthorizedParty,
    #[error("it has expired")]
    Expired,
    #[error("it was issued in the future")]
    IssuedInFuture,
    #[error("its nonce doesn't match this sign-in")]
    Nonce,
    #[error("its subject doesn't match the identity provider's userinfo")]
    Subject,
}
