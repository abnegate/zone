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
    fn an_unknown_status_is_other() {
        assert_eq!(parse("frozen"), SubscriptionStatus::Other);
    }
}
