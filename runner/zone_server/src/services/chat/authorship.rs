//! The metadata a client may not write, because the console reads it as
//! "nobody in this chat typed this message".

use serde_json::Value;

/// Set only by the reminder and auto-project paths that write a message
/// without a person behind it.
pub const SERVER_KEYS: [&str; 2] = ["source", "reminder_id"];

/// The first server-only key a client tried to set, if any.
pub fn claimed(metadata: Option<&Value>) -> Option<&'static str> {
    let fields = metadata?.as_object()?;
    SERVER_KEYS
        .into_iter()
        .find(|key| fields.contains_key(*key))
}

pub fn refusal(key: &str) -> String {
    format!("Message metadata may not set `{key}`")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_client_claiming_an_automation_is_caught() {
        assert_eq!(
            claimed(Some(&json!({"source": "reminder"}))),
            Some("source")
        );
        assert_eq!(
            claimed(Some(&json!({"reminder_id": "x"}))),
            Some("reminder_id")
        );
        assert_eq!(claimed(Some(&json!({"source": null}))), Some("source"));
    }

    #[test]
    fn ordinary_client_metadata_passes() {
        assert_eq!(claimed(None), None);
        assert_eq!(claimed(Some(&Value::Null)), None);
        assert_eq!(claimed(Some(&json!(["source"]))), None);
        assert_eq!(
            claimed(Some(&json!({"attachments": [], "web_search": true}))),
            None
        );
    }
}
