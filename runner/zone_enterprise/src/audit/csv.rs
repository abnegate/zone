use serde_json::Value;
use uuid::Uuid;

use super::Event;

pub const HEADER: &str = "id,created_at,organization_id,workspace_id,actor_id,actor_email,action,resource_type,resource_id,ip_address,user_agent,old_values,new_values\r\n";

const DELIMITER: char = ',';
const QUOTE: char = '"';
const TERMINATOR: &str = "\r\n";
const FORMULA_PREFIXES: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];
const NEUTRALISER: char = '\'';

pub fn row(event: &Event) -> String {
    let fields = [
        event.id.to_string(),
        event.timestamp(),
        identifier(event.organization_id),
        identifier(event.workspace_id),
        identifier(event.actor_id),
        field(event.actor_email.as_deref().unwrap_or_default()),
        field(&event.action),
        field(&event.resource_type),
        identifier(event.resource_id),
        field(event.ip_address.as_deref().unwrap_or_default()),
        field(event.user_agent.as_deref().unwrap_or_default()),
        field(&json(event.old_values.as_ref())),
        field(&json(event.new_values.as_ref())),
    ];
    let mut row = fields.join(&DELIMITER.to_string());
    row.push_str(TERMINATOR);
    row
}

fn identifier(value: Option<Uuid>) -> String {
    value.map(|id| id.to_string()).unwrap_or_default()
}

fn json(value: Option<&Value>) -> String {
    value.map(Value::to_string).unwrap_or_default()
}

fn field(value: &str) -> String {
    let neutralised = value.starts_with(FORMULA_PREFIXES);
    let quoted = neutralised || value.contains([DELIMITER, QUOTE, '\r', '\n']);
    if !quoted {
        return value.to_owned();
    }
    let mut field = String::with_capacity(value.len() + 3);
    field.push(QUOTE);
    if neutralised {
        field.push(NEUTRALISER);
    }
    for character in value.chars() {
        if character == QUOTE {
            field.push(QUOTE);
        }
        field.push(character);
    }
    field.push(QUOTE);
    field
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::audit::event::fixture;

    fn parse(row: &str) -> Vec<String> {
        let row = row
            .strip_suffix(TERMINATOR)
            .expect("the row ends with CRLF");
        let mut fields = vec![String::new()];
        let mut quoted = false;
        let mut characters = row.chars().peekable();
        while let Some(character) = characters.next() {
            match (quoted, character) {
                (true, QUOTE) if characters.peek() == Some(&QUOTE) => {
                    characters.next();
                    fields.last_mut().unwrap().push(QUOTE);
                }
                (_, QUOTE) => quoted = !quoted,
                (false, DELIMITER) => fields.push(String::new()),
                (_, character) => fields.last_mut().unwrap().push(character),
            }
        }
        assert!(!quoted, "unterminated quote in {row}");
        fields
    }

    #[test]
    fn header_names_every_column_and_ends_with_crlf() {
        assert!(HEADER.ends_with("\r\n"));
        assert_eq!(
            parse(HEADER),
            [
                "id",
                "created_at",
                "organization_id",
                "workspace_id",
                "actor_id",
                "actor_email",
                "action",
                "resource_type",
                "resource_id",
                "ip_address",
                "user_agent",
                "old_values",
                "new_values",
            ]
        );
    }

    #[test]
    fn writes_the_fixture_row() {
        assert_eq!(
            row(&fixture::event()),
            concat!(
                "0192f3a4-5b6c-7d8e-9f01-23456789abcd,2026-11-17T09:30:05.123456Z,",
                "11111111-1111-4111-8111-111111111111,22222222-2222-4222-8222-222222222222,",
                "33333333-3333-4333-8333-333333333333,ada@example.com,member.removed,member,",
                "44444444-4444-4444-8444-444444444444,192.0.2.10,Mozilla/5.0,",
                "\"{\"\"role\"\":\"\"admin\"\"}\",\r\n",
            )
        );
    }

    #[test]
    fn writes_absent_values_as_empty_fields() {
        let fields = parse(&row(&fixture::bare()));
        assert_eq!(fields.len(), 13);
        for index in [2, 3, 4, 5, 8, 9, 10, 11, 12] {
            assert_eq!(fields[index], "", "column {index}");
        }
    }

    #[test]
    fn quotes_delimiters_quotes_and_line_breaks() {
        assert_eq!(field("plain"), "plain");
        assert_eq!(field(""), "");
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field("one\ntwo"), "\"one\ntwo\"");
        assert_eq!(field("one\r\ntwo"), "\"one\r\ntwo\"");
    }

    #[test]
    fn neutralises_formula_prefixes() {
        for (value, expected) in [
            ("=SUM(A1)", "\"'=SUM(A1)\""),
            ("+1", "\"'+1\""),
            ("-1", "\"'-1\""),
            ("@cmd", "\"'@cmd\""),
            ("\tindent", "\"'\tindent\""),
            ("\rreturn", "\"'\rreturn\""),
            ("=\"quoted\"", "\"'=\"\"quoted\"\"\""),
        ] {
            assert_eq!(field(value), expected, "{value:?}");
        }
        assert_eq!(field("a=b"), "a=b");
    }

    #[test]
    fn round_trips_hostile_values() {
        let mut event = fixture::event();
        event.actor_email = Some("=HYPERLINK(\"http://x\")".to_owned());
        event.user_agent = Some("agent, \"quoted\"\r\nnext".to_owned());
        event.new_values = Some(json!({"note": "a,b\n\"c\""}));
        let fields = parse(&row(&event));
        assert_eq!(fields.len(), 13);
        assert_eq!(fields[5], "'=HYPERLINK(\"http://x\")");
        assert_eq!(fields[10], "agent, \"quoted\"\r\nnext");
        assert_eq!(
            serde_json::from_str::<Value>(&fields[12]).unwrap(),
            json!({"note": "a,b\n\"c\""})
        );
    }
}
