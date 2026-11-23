use crate::db::scim::{GroupRecord, Mapping, Outcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remapped {
    pub previous: Mapping,
    pub group: GroupRecord,
    pub outcomes: Vec<Outcome>,
}
