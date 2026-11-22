use crate::db::suspensions::Suspension;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deactivation {
    Deactivated(Suspension),
    LastOwner,
}
