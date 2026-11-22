use crate::db::fleet::state::State;

/// A state change that ends no live work. Revoking also stops the runner's work, so it goes
/// through `db::fleet::revocation` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Activate,
    Cordon,
}

impl Transition {
    pub fn state(self) -> State {
        match self {
            Self::Activate => State::Active,
            Self::Cordon => State::Cordoned,
        }
    }
}
