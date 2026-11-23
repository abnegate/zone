use uuid::Uuid;

use crate::db::scim::Outcome;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub affected: Vec<Uuid>,
    pub outcomes: Vec<Outcome>,
}
