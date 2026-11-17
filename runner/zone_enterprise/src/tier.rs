use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Team,
    Enterprise,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Tier::Team).unwrap(), r#""team""#);
        assert_eq!(
            serde_json::to_string(&Tier::Enterprise).unwrap(),
            r#""enterprise""#
        );
    }

    #[test]
    fn refuses_unknown_tier() {
        assert!(serde_json::from_str::<Tier>(r#""Team""#).is_err());
        assert!(serde_json::from_str::<Tier>(r#""community""#).is_err());
    }
}
