use crate::sso::saml::malformed::Malformed;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Refusal {
    #[error(transparent)]
    Malformed(#[from] Malformed),
    #[error("it isn't signed")]
    Unsigned,
    #[error("an element carries more than one signature")]
    SignatureCount,
    #[error("its signature doesn't reference the element it signs")]
    Reference,
    #[error(
        "its signature uses transforms other than enveloped-signature followed by exclusive canonicalization"
    )]
    Transform,
    #[error("its signature or digest algorithm isn't allowed")]
    Algorithm,
    #[error("its signed content doesn't match its digest")]
    Digest,
    #[error("its signature doesn't verify against the identity provider's certificates")]
    Signature,
    #[error("two of its elements share an ID")]
    DuplicateId,
    #[error("it doesn't contain exactly one assertion")]
    AssertionCount,
    #[error("it's encrypted, and Zone doesn't accept encrypted assertions")]
    Encrypted,
    #[error("it isn't a SAML 2.0 message")]
    Version,
    #[error("it was issued by a different identity provider")]
    Issuer,
    #[error("it was sent to a different destination")]
    Destination,
    #[error("it doesn't answer this sign-in request")]
    InResponseTo,
    #[error("it was addressed to a different recipient")]
    Recipient,
    #[error("it's meant for a different service provider")]
    Audience,
    #[error("it has no valid bearer subject confirmation")]
    Confirmation,
    #[error("it carries a condition Zone doesn't understand")]
    Condition,
    #[error("it isn't valid yet")]
    NotYetValid,
    #[error("it has expired")]
    Expired,
    #[error("it was issued in the future")]
    IssuedInFuture,
    #[error("it doesn't name exactly one subject")]
    Subject,
    #[error("its subject uses a transient name identifier")]
    NameIdFormat,
    #[error("it was already used")]
    Replayed,
    #[error("it has no authentication statement")]
    Statement,
}
