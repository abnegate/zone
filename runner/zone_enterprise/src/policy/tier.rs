use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Read,
    Write,
    Host,
    Outward,
}

impl Tier {
    pub const ALL: [Self; 4] = [Self::Read, Self::Write, Self::Host, Self::Outward];

    pub const CEILINGS: [Self; 2] = [Self::Write, Self::Host];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Host => "host",
            Self::Outward => "outward",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_lowercase() {
        let values: Vec<String> = Tier::ALL
            .iter()
            .map(|tier| serde_json::to_string(tier).unwrap())
            .collect();
        assert_eq!(
            values,
            [r#""read""#, r#""write""#, r#""host""#, r#""outward""#]
        );
        for tier in Tier::ALL {
            assert_eq!(serde_json::to_value(tier).unwrap(), tier.as_str());
        }
    }

    #[test]
    fn refuses_unknown_values() {
        for value in [r#""Write""#, r#""none""#, r#""""#] {
            assert!(serde_json::from_str::<Tier>(value).is_err(), "{value}");
        }
    }

    #[test]
    fn orders_like_the_tool_tiers() {
        assert!(Tier::Read < Tier::Write);
        assert!(Tier::Write < Tier::Host);
        assert!(Tier::Host < Tier::Outward);
    }

    #[test]
    fn only_write_and_host_are_ceilings() {
        assert_eq!(Tier::CEILINGS, [Tier::Write, Tier::Host]);
    }
}
