use super::Hunk;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub hunks: Vec<Hunk>,
    pub additions: u32,
    pub deletions: u32,
    pub truncated: bool,
}
