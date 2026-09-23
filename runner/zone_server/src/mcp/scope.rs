//! Who a served turn acts for, and how far it may go.

use uuid::Uuid;

use crate::agent::ApprovalPolicy;

/// Everything about a turn but its tools.
#[derive(Clone)]
pub struct Scope {
    pub workspace: Uuid,
    /// The chat, or the task run.
    pub chat: Uuid,
    /// Who acts in it: the chat's reader, or whoever started the run. A run
    /// nobody started has nobody.
    pub user: Option<Uuid>,
    pub approval: ApprovalPolicy,
    /// How many calls the agent may make: as many as zone's own loop would
    /// have made for the same turn.
    pub calls: usize,
}
