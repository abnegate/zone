use uuid::Uuid;

use super::applied::Applied;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub user_id: Uuid,
    pub applied: Applied,
}
