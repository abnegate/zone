/// What became of an answering task's claim on its chat's serving key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// The key names this task now, written as this payload.
    Taken(String),
    /// Another replica's answering task keeps the key: it holds the chat, or
    /// both are only waiting out their grace and it got there first.
    Held,
    /// Valkey could not say, so the replica goes by what it holds itself.
    Unknown,
}
