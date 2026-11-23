use crate::db::fleet::runners::Runner;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    Removed(Runner),
    Online,
    /// Offline, but a live task run or chat lease is still recorded against it.
    Busy,
    Missing,
}
