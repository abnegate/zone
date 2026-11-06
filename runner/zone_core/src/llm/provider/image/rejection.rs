/// Why an attached image cannot be shown to an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Rejection {
    #[error("it is not a PNG, JPEG, GIF or WebP image")]
    Unsupported,
    #[error("its data could not be decoded as the image it says it is")]
    Malformed,
    #[error("it is stored where only Zone can read it")]
    Unreachable,
    #[error("it is a link, and codex opens images only from files")]
    Linked,
    #[error("it is larger than the 10 MB an image may be")]
    Oversized,
    #[error("the conversation already carries the 100 images a turn may hold")]
    Excess,
    #[error("the conversation's images together pass the 128 MB a turn may carry")]
    Crowded,
}
