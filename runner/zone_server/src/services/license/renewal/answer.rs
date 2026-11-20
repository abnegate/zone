#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Renewed(String),
    Current,
    Ended,
    Refused,
    Unreachable,
    Malformed,
}
