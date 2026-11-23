#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignOut {
    Revoked { sessions: i64 },
    Shared,
}
