/// Why an inbound envelope was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Rejection {
    #[error("the envelope is shorter than its framing")]
    Truncated,
    #[error("the envelope has an unknown version")]
    Version,
    #[error("the envelope has an unknown kind")]
    Kind,
    #[error("the envelope's signature does not verify")]
    Signature,
}
