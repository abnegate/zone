use crate::db::fleet::policy::Policy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub previous: Policy,
    pub current: Policy,
}

impl Saved {
    pub fn changed(&self) -> bool {
        self.previous.minimum != self.current.minimum
    }
}
