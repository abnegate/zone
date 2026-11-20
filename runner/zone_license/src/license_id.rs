use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const NAMESPACE: Uuid = Uuid::from_u128(0x16ea_fa97_dd78_4dff_8550_01e0_45e0_de9d);

pub fn derive(subscription: &str, period_end: DateTime<Utc>, seats: u32) -> Uuid {
    let name = format!("{subscription}/{}/{seats}", period_end.timestamp());
    Uuid::new_v5(&NAMESPACE, name.as_bytes())
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use uuid::Version;

    use super::*;

    const SUBSCRIPTION: &str = "sub_01HZX0Q7M8";

    fn period_end() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2027-11-01T00:00:00Z")
            .unwrap()
            .to_utc()
    }

    #[test]
    fn the_namespace_is_a_random_uuid() {
        assert_eq!(NAMESPACE.get_version(), Some(Version::Random));
    }

    #[test]
    fn is_a_name_based_uuid() {
        assert_eq!(
            derive(SUBSCRIPTION, period_end(), 25).get_version(),
            Some(Version::Sha1)
        );
    }

    #[test]
    fn is_deterministic() {
        assert_eq!(
            derive(SUBSCRIPTION, period_end(), 25),
            derive(SUBSCRIPTION, period_end(), 25)
        );
    }

    #[test]
    fn hashes_the_documented_name() {
        assert_eq!(
            derive(SUBSCRIPTION, period_end(), 25),
            Uuid::new_v5(&NAMESPACE, b"sub_01HZX0Q7M8/1825027200/25")
        );
    }

    #[test]
    fn changes_with_the_seats() {
        assert_ne!(
            derive(SUBSCRIPTION, period_end(), 25),
            derive(SUBSCRIPTION, period_end(), 26)
        );
    }

    #[test]
    fn changes_with_the_period_end() {
        assert_ne!(
            derive(SUBSCRIPTION, period_end(), 25),
            derive(SUBSCRIPTION, period_end() + TimeDelta::seconds(1), 25)
        );
    }

    #[test]
    fn changes_with_the_subscription() {
        assert_ne!(
            derive(SUBSCRIPTION, period_end(), 25),
            derive("sub_01HZX0Q7M9", period_end(), 25)
        );
    }
}
