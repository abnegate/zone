#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the entry {entry:?} is not {expected}")]
pub struct Unparsable {
    pub entry: String,
    pub expected: &'static str,
}
