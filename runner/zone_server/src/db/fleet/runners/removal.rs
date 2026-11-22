use crate::db::fleet::runners::Runner;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    Removed(Runner),
    Online,
    Missing,
}
