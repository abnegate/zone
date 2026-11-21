use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Incomplete,
    IncompleteExpired,
    Trialing,
    Active,
    PastDue,
    Canceled,
    Unpaid,
    Paused,
    #[serde(other)]
    Other,
}

impl SubscriptionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incomplete => "incomplete",
            Self::IncompleteExpired => "incomplete_expired",
            Self::Trialing => "trialing",
            Self::Active => "active",
            Self::PastDue => "past_due",
            Self::Canceled => "canceled",
            Self::Unpaid => "unpaid",
            Self::Paused => "paused",
            Self::Other => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> SubscriptionStatus {
        serde_json::from_str(&format!("\"{text}\"")).unwrap()
    }

    #[test]
    fn parses_every_documented_status() {
        assert_eq!(parse("incomplete"), SubscriptionStatus::Incomplete);
        assert_eq!(
            parse("incomplete_expired"),
            SubscriptionStatus::IncompleteExpired
        );
        assert_eq!(parse("trialing"), SubscriptionStatus::Trialing);
        assert_eq!(parse("active"), SubscriptionStatus::Active);
        assert_eq!(parse("past_due"), SubscriptionStatus::PastDue);
        assert_eq!(parse("canceled"), SubscriptionStatus::Canceled);
        assert_eq!(parse("unpaid"), SubscriptionStatus::Unpaid);
        assert_eq!(parse("paused"), SubscriptionStatus::Paused);
    }

    #[test]
    fn names_each_status_as_polar_does() {
        for text in [
            "incomplete",
            "incomplete_expired",
            "trialing",
            "active",
            "past_due",
            "canceled",
            "unpaid",
            "paused",
        ] {
            assert_eq!(parse(text).as_str(), text);
        }
        assert_eq!(SubscriptionStatus::Other.as_str(), "other");
    }

    #[test]
    fn an_unknown_status_is_other() {
        assert_eq!(parse("frozen"), SubscriptionStatus::Other);
    }
}
