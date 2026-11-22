use crate::db::suspensions::Successor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    Removed(Vec<Successor>),
    LastOwner,
    Missing,
}
