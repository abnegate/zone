use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const SERVER: &str = "server";

const DATE_PATH: &str = "%Y/%m/%d";
const TIME: &str = "%H%M%S";
const EXTENSION: &str = "ndjson";

pub fn object_key(
    prefix: &str,
    organization_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    first: Uuid,
) -> String {
    let owner = organization_id.map_or_else(|| SERVER.to_owned(), |id| id.to_string());
    format!(
        "{prefix}{owner}/{}/{}Z-{first}.{EXTENSION}",
        created_at.format(DATE_PATH),
        created_at.format(TIME)
    )
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    const ORGANIZATION: Uuid = Uuid::from_u128(0x0a);
    const FIRST: Uuid = Uuid::from_u128(0x01);

    fn created_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 4, 5, 6, 7).unwrap()
    }

    #[test]
    fn the_server_label_is_the_wire_contract() {
        assert_eq!(SERVER, "server");
    }

    #[test]
    fn keys_an_organization_object_by_day_and_first_event() {
        assert_eq!(
            object_key("audit/", Some(ORGANIZATION), created_at(), FIRST),
            "audit/00000000-0000-0000-0000-00000000000a/2026/03/04/050607Z-00000000-0000-0000-0000-000000000001.ndjson"
        );
    }

    #[test]
    fn keys_a_server_object_under_the_server_label() {
        assert_eq!(
            object_key("audit/", None, created_at(), FIRST),
            "audit/server/2026/03/04/050607Z-00000000-0000-0000-0000-000000000001.ndjson"
        );
    }

    #[test]
    fn an_empty_prefix_starts_at_the_owner() {
        assert_eq!(
            object_key("", None, created_at(), FIRST),
            "server/2026/03/04/050607Z-00000000-0000-0000-0000-000000000001.ndjson"
        );
    }

    #[test]
    fn drops_fractional_seconds() {
        let precise = created_at() + chrono::Duration::milliseconds(987);
        assert_eq!(
            object_key("", None, precise, FIRST),
            object_key("", None, created_at(), FIRST)
        );
    }

    #[test]
    fn the_same_batch_always_gets_the_same_key() {
        let key = object_key("audit/", Some(ORGANIZATION), created_at(), FIRST);
        assert_eq!(
            key,
            object_key("audit/", Some(ORGANIZATION), created_at(), FIRST)
        );
        assert_ne!(
            key,
            object_key(
                "audit/",
                Some(ORGANIZATION),
                created_at(),
                Uuid::from_u128(2)
            )
        );
    }
}
