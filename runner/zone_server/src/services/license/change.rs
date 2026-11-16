use zone_enterprise::License;

use super::stored::Stored;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub previous: Stored,
    pub current: License,
}
