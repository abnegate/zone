use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const VERSION: u32 = 1;

const TIMESTAMP_FORMAT: SecondsFormat = SecondsFormat::Micros;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub version: u32,
    #[serde(serialize_with = "serialize_timestamp")]
    pub created_at: DateTime<Utc>,
    pub organization_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub actor_email: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub old_values: Option<Value>,
    pub new_values: Option<Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

impl Event {
    pub fn timestamp(&self) -> String {
        self.created_at.to_rfc3339_opts(TIMESTAMP_FORMAT, true)
    }
}

fn serialize_timestamp<S: serde::Serializer>(
    value: &DateTime<Utc>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_rfc3339_opts(TIMESTAMP_FORMAT, true))
}

#[cfg(test)]
pub(crate) mod fixture {
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;
    use uuid::Uuid;

    use super::{Event, VERSION};

    pub(crate) const ID: &str = "0192f3a4-5b6c-7d8e-9f01-23456789abcd";
    pub(crate) const ORGANIZATION: &str = "11111111-1111-4111-8111-111111111111";
    pub(crate) const WORKSPACE: &str = "22222222-2222-4222-8222-222222222222";
    pub(crate) const ACTOR: &str = "33333333-3333-4333-8333-333333333333";
    pub(crate) const RESOURCE: &str = "44444444-4444-4444-8444-444444444444";

    pub(crate) fn event() -> Event {
        Event {
            id: Uuid::parse_str(ID).unwrap(),
            version: VERSION,
            created_at: Utc.with_ymd_and_hms(2026, 11, 17, 9, 30, 5).unwrap()
                + Duration::microseconds(123_456),
            organization_id: Some(Uuid::parse_str(ORGANIZATION).unwrap()),
            workspace_id: Some(Uuid::parse_str(WORKSPACE).unwrap()),
            actor_id: Some(Uuid::parse_str(ACTOR).unwrap()),
            actor_email: Some("ada@example.com".to_owned()),
            action: "member.removed".to_owned(),
            resource_type: "member".to_owned(),
            resource_id: Some(Uuid::parse_str(RESOURCE).unwrap()),
            old_values: Some(json!({"role": "admin"})),
            new_values: None,
            ip_address: Some("192.0.2.10".to_owned()),
            user_agent: Some("Mozilla/5.0".to_owned()),
        }
    }

    pub(crate) fn bare() -> Event {
        Event {
            organization_id: None,
            workspace_id: None,
            actor_id: None,
            actor_email: None,
            resource_id: None,
            old_values: None,
            new_values: None,
            ip_address: None,
            user_agent: None,
            ..event()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: [&str; 14] = [
        "id",
        "version",
        "created_at",
        "organization_id",
        "workspace_id",
        "actor_id",
        "actor_email",
        "action",
        "resource_type",
        "resource_id",
        "old_values",
        "new_values",
        "ip_address",
        "user_agent",
    ];

    #[test]
    fn version_is_one() {
        assert_eq!(VERSION, 1);
    }

    #[test]
    fn serializes_keys_in_schema_order() {
        let json = serde_json::to_string(&fixture::event()).unwrap();
        let mut cursor = 0;
        for key in KEYS {
            let needle = format!("\"{key}\":");
            let offset = json[cursor..]
                .find(&needle)
                .unwrap_or_else(|| panic!("{key} is missing or out of order in {json}"));
            cursor += offset + needle.len();
        }
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value.as_object().unwrap().len(), KEYS.len());
    }

    #[test]
    fn serializes_the_fixture_exactly() {
        assert_eq!(
            serde_json::to_string(&fixture::event()).unwrap(),
            concat!(
                r#"{"id":"0192f3a4-5b6c-7d8e-9f01-23456789abcd","version":1,"#,
                r#""created_at":"2026-11-17T09:30:05.123456Z","#,
                r#""organization_id":"11111111-1111-4111-8111-111111111111","#,
                r#""workspace_id":"22222222-2222-4222-8222-222222222222","#,
                r#""actor_id":"33333333-3333-4333-8333-333333333333","#,
                r#""actor_email":"ada@example.com","action":"member.removed","#,
                r#""resource_type":"member","#,
                r#""resource_id":"44444444-4444-4444-8444-444444444444","#,
                r#""old_values":{"role":"admin"},"new_values":null,"#,
                r#""ip_address":"192.0.2.10","user_agent":"Mozilla/5.0"}"#,
            )
        );
    }

    #[test]
    fn keeps_absent_values_as_null() {
        let value = serde_json::to_value(fixture::bare()).unwrap();
        for key in [
            "organization_id",
            "workspace_id",
            "actor_id",
            "actor_email",
            "resource_id",
            "old_values",
            "new_values",
            "ip_address",
            "user_agent",
        ] {
            assert_eq!(value[key], Value::Null, "{key}");
        }
    }

    #[test]
    fn writes_microsecond_utc_timestamps() {
        let mut event = fixture::event();
        event.created_at = DateTime::parse_from_rfc3339("2026-01-02T03:04:05+02:00")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(event.timestamp(), "2026-01-02T01:04:05.000000Z");
        assert_eq!(
            serde_json::to_value(&event).unwrap()["created_at"],
            "2026-01-02T01:04:05.000000Z"
        );
    }

    #[test]
    fn round_trips() {
        for event in [fixture::event(), fixture::bare()] {
            let json = serde_json::to_vec(&event).unwrap();
            assert_eq!(serde_json::from_slice::<Event>(&json).unwrap(), event);
        }
    }
}
