use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    pub fn contains(&self, client_id: &str) -> bool {
        match self {
            Self::One(audience) => audience == client_id,
            Self::Many(audiences) => audiences.iter().any(|audience| audience == client_id),
        }
    }

    pub fn several(&self) -> bool {
        matches!(self, Self::Many(audiences) if audiences.len() > 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_string_or_an_array() {
        assert_eq!(
            serde_json::from_str::<Audience>(r#""zone""#).unwrap(),
            Audience::One("zone".into())
        );
        assert_eq!(
            serde_json::from_str::<Audience>(r#"["zone","other"]"#).unwrap(),
            Audience::Many(vec!["zone".into(), "other".into()])
        );
    }

    #[test]
    fn contains_matches_exactly() {
        assert!(Audience::One("zone".into()).contains("zone"));
        assert!(!Audience::One("zone".into()).contains("Zone"));
        assert!(Audience::Many(vec!["other".into(), "zone".into()]).contains("zone"));
        assert!(!Audience::Many(Vec::new()).contains("zone"));
    }

    #[test]
    fn several_means_more_than_one() {
        assert!(!Audience::One("zone".into()).several());
        assert!(!Audience::Many(vec!["zone".into()]).several());
        assert!(Audience::Many(vec!["zone".into(), "other".into()]).several());
    }
}
