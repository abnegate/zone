use crate::db::fleet::runners::Runner;
use crate::db::fleet::state::State;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Updated {
    pub previous: State,
    pub runner: Runner,
}

impl Updated {
    pub fn changed(&self) -> bool {
        self.previous != self.runner.state
    }
}
