use uuid::Uuid;

/// A chat as one bus instance answers it, which tells apart the answering
/// tasks of replicas sharing a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Answered {
    pub instance: Uuid,
    pub chat: Uuid,
}
