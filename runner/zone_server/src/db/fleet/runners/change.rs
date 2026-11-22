use uuid::Uuid;

use crate::db::fleet::minimum::Minimum;

/// A prospective change that may reduce the runners able to take work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Cordon(Uuid),
    Revoke(Uuid),
    Minimum(Option<Minimum>),
}
