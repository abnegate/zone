use uuid::Uuid;

/// A task run a runner may pick up because the runner that admitted it couldn't take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct Candidate {
    pub run: Uuid,
    pub task: Uuid,
}
