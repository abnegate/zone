/// How a turn run with nobody connected went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// It reached the chat, whatever it made of the message there.
    Ran,
    /// Fleet policy refused this runner the chat's lease, so nothing reached the chat and the
    /// work is still owed.
    Refused,
}
