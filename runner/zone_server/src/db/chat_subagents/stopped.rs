use sqlx::types::Json;
use zone_core::llm::SubagentStep;

use super::SubagentRow;

/// A subagent a stop settled, with the steps it failed along with it.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Stopped {
    #[sqlx(flatten)]
    pub subagent: SubagentRow,
    pub failed: Json<Vec<SubagentStep>>,
}
