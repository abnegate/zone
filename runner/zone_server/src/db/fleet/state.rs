#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Active,
    Cordoned,
    Revoked,
}

impl State {
    pub const ALL: [Self; 3] = [Self::Active, Self::Cordoned, Self::Revoked];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Cordoned => "cordoned",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_are_the_stored_names() {
        assert_eq!(State::Active.as_str(), "active");
        assert_eq!(State::Cordoned.as_str(), "cordoned");
        assert_eq!(State::Revoked.as_str(), "revoked");
    }

    #[test]
    fn every_state_parses_back() {
        for state in State::ALL {
            assert_eq!(State::parse(state.as_str()), Some(state));
        }
        assert_eq!(State::parse("draining"), None);
        assert_eq!(State::parse("Active"), None);
    }
}
