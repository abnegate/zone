use crate::db::scim::{GroupRecord, Mapping, Outcome};

/// A group's new mapping, what it replaced, and what applying it did to each member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remapped {
    pub previous: Mapping,
    pub group: GroupRecord,
    pub outcomes: Vec<Outcome>,
}
