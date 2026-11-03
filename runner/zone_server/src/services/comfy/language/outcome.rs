use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub path: PathBuf,
    pub examples: usize,
}
