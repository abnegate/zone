use super::Event;

pub const APP_NAME: &str = "zone";
pub const STRUCTURED_DATA_ID: &str = "zone@32473";
pub const UDP_BUDGET: usize = 65_507;

const PROTOCOL_VERSION: u8 = 1;
const NIL: &str = "-";
const BOM: char = '\u{FEFF}';
const HOSTNAME_LENGTH: usize = 255;
const MESSAGE_ID_LENGTH: usize = 32;
const PRINTABLE: std::ops::RangeInclusive<char> = '!'..='~';
const ESCAPED: [char; 3] = ['"', '\\', ']'];
const WARNING_SUFFIX: &str = "_denied";
const NOTICE_SUFFIXES: [&str; 3] = [".deleted", ".removed", ".blocked"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header<'a> {
    pub facility: u8,
    pub hostname: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Severity {
    Warning = 4,
    Notice = 5,
    Informational = 6,
}

pub fn severity(action: &str) -> Severity {
    if action.ends_with(WARNING_SUFFIX) {
        Severity::Warning
    } else if NOTICE_SUFFIXES
        .iter()
        .any(|suffix| action.ends_with(suffix))
    {
        Severity::Notice
    } else {
        Severity::Informational
    }
}

pub fn message(event: &Event, header: &Header, budget: Option<usize>) -> String {
    let priority = u16::from(header.facility) * 8 + severity(&event.action) as u16;
    let body = serde_json::to_string(event).expect("an event always serializes to JSON");
    let mut message = format!(
        "<{priority}>{PROTOCOL_VERSION} {timestamp} {hostname} {APP_NAME} {NIL} {identifier} {data} {BOM}{body}",
        timestamp = event.timestamp(),
        hostname = token(header.hostname, HOSTNAME_LENGTH),
        identifier = token(&event.action, MESSAGE_ID_LENGTH),
        data = structured_data(event),
    );
    if let Some(budget) = budget {
        message.truncate(message.floor_char_boundary(budget));
    }
    message
}

pub fn octet_counted(message: &str) -> Vec<u8> {
    format!("{} {message}", message.len()).into_bytes()
}

fn token(value: &str, length: usize) -> String {
    let token: String = value
        .chars()
        .filter(|character| PRINTABLE.contains(character))
        .take(length)
        .collect();
    if token.is_empty() {
        NIL.to_owned()
    } else {
        token
    }
}

fn structured_data(event: &Event) -> String {
    let parameters = [
        ("id", Some(event.id.to_string())),
        (
            "organization",
            event.organization_id.map(|id| id.to_string()),
        ),
        ("workspace", event.workspace_id.map(|id| id.to_string())),
        ("actor", event.actor_id.map(|id| id.to_string())),
        ("actor_email", event.actor_email.clone()),
        ("resource_type", Some(event.resource_type.clone())),
        ("resource", event.resource_id.map(|id| id.to_string())),
        ("ip", event.ip_address.clone()),
    ];
    let mut element = format!("[{STRUCTURED_DATA_ID}");
    for (name, value) in parameters {
        if let Some(value) = value {
            element.push(' ');
            element.push_str(name);
            element.push_str("=\"");
            element.push_str(&escape(&value));
            element.push('"');
        }
    }
    element.push(']');
    element
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if ESCAPED.contains(&character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::event::fixture;

    const HOSTNAME: &str = "zone-1";
    const HEADER: Header<'static> = Header {
        facility: 13,
        hostname: HOSTNAME,
    };
    const BOM_BYTES: [u8; 3] = [0xEF, 0xBB, 0xBF];

    fn body(message: &str) -> &str {
        let start = message.find(BOM).expect("the message carries a BOM");
        &message[start + BOM.len_utf8()..]
    }

    fn fields(message: &str) -> Vec<&str> {
        message.splitn(8, ' ').collect()
    }

    #[test]
    fn writes_the_fixture_line() {
        let event = fixture::event();
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            message(&event, &HEADER, None),
            format!(
                concat!(
                    "<109>1 2026-11-17T09:30:05.123456Z zone-1 zone - member.removed ",
                    "[zone@32473 id=\"0192f3a4-5b6c-7d8e-9f01-23456789abcd\" ",
                    "organization=\"11111111-1111-4111-8111-111111111111\" ",
                    "workspace=\"22222222-2222-4222-8222-222222222222\" ",
                    "actor=\"33333333-3333-4333-8333-333333333333\" ",
                    "actor_email=\"ada@example.com\" resource_type=\"member\" ",
                    "resource=\"44444444-4444-4444-8444-444444444444\" ip=\"192.0.2.10\"] ",
                    "\u{FEFF}{}",
                ),
                json
            )
        );
    }

    #[test]
    fn header_follows_the_rfc_5424_grammar() {
        let message = message(&fixture::event(), &HEADER, None);
        let fields = fields(&message);
        let (priority, version) = fields[0]
            .strip_prefix('<')
            .and_then(|rest| rest.split_once('>'))
            .unwrap();
        let priority: u16 = priority.parse().unwrap();
        assert!(priority <= 191);
        assert_eq!(version, "1");
        assert!(chrono::DateTime::parse_from_rfc3339(fields[1]).is_ok());
        assert_eq!(fields[2], HOSTNAME);
        assert_eq!(fields[3], APP_NAME);
        assert_eq!(fields[4], NIL);
        assert_eq!(fields[5], "member.removed");
        for field in &fields[1..6] {
            assert!(
                field
                    .chars()
                    .all(|character| PRINTABLE.contains(&character))
            );
        }
        assert_eq!(fields[6], format!("[{STRUCTURED_DATA_ID}"));
    }

    #[test]
    fn computes_the_priority_from_facility_and_severity() {
        let mut event = fixture::event();
        for (facility, action, priority) in [
            (0, "user.login", "<6>"),
            (13, "user.login", "<110>"),
            (13, "agent_policy.tool_denied", "<108>"),
            (16, "workspace.deleted", "<133>"),
            (23, "user.login", "<190>"),
        ] {
            event.action = action.to_owned();
            let header = Header {
                facility,
                hostname: HOSTNAME,
            };
            assert!(
                message(&event, &header, None).starts_with(&format!("{priority}1 ")),
                "{facility} {action}"
            );
        }
    }

    #[test]
    fn maps_actions_to_severities() {
        for (action, expected) in [
            ("agent_policy.turn_denied", Severity::Warning),
            ("agent_policy.tool_denied", Severity::Warning),
            ("user.deleted", Severity::Notice),
            ("member.removed", Severity::Notice),
            ("device.blocked", Severity::Notice),
            ("user.login", Severity::Informational),
            ("device.allowed", Severity::Informational),
            ("denied.created", Severity::Informational),
            ("license.removed_by", Severity::Informational),
        ] {
            assert_eq!(severity(action), expected, "{action}");
        }
        assert_eq!(Severity::Warning as u8, 4);
        assert_eq!(Severity::Notice as u8, 5);
        assert_eq!(Severity::Informational as u8, 6);
    }

    #[test]
    fn escapes_structured_data_values() {
        let mut event = fixture::bare();
        event.actor_email = Some(r#"a"b\c]d[e"#.to_owned());
        let message = message(&event, &HEADER, None);
        assert!(
            message.contains(r#" actor_email="a\"b\\c\]d[e" "#),
            "{message}"
        );
    }

    #[test]
    fn omits_absent_structured_data_parameters() {
        let message = message(&fixture::bare(), &HEADER, None);
        assert!(message.contains(&format!(
            "[{STRUCTURED_DATA_ID} id=\"{}\" resource_type=\"member\"] {BOM}",
            fixture::ID
        )));
    }

    #[test]
    fn cuts_the_message_id_to_32_printable_characters() {
        let mut event = fixture::event();
        event.action = "a very long action\u{e9}name.that_goes_on_and_on".to_owned();
        let message = message(&event, &HEADER, None);
        let identifier = fields(&message)[5];
        assert_eq!(identifier, "averylongactionname.that_goes_on");
        assert_eq!(identifier.len(), 32);
    }

    #[test]
    fn falls_back_to_nil_for_empty_tokens() {
        let mut event = fixture::event();
        event.action = " \u{e9}\t".to_owned();
        let header = Header {
            facility: 13,
            hostname: " ",
        };
        let message = message(&event, &header, None);
        let fields = fields(&message);
        assert_eq!(fields[2], NIL);
        assert_eq!(fields[5], NIL);
    }

    #[test]
    fn cuts_the_hostname_to_255_characters() {
        let hostname = "h".repeat(300);
        let header = Header {
            facility: 13,
            hostname: &hostname,
        };
        let message = message(&fixture::event(), &header, None);
        assert_eq!(fields(&message)[2], "h".repeat(255));
    }

    #[test]
    fn prefixes_the_json_body_with_a_bom() {
        let event = fixture::event();
        let message = message(&event, &HEADER, None);
        let data_end = message.find("] ").unwrap() + 2;
        assert_eq!(&message.as_bytes()[data_end..data_end + 3], BOM_BYTES);
        let parsed: Event = serde_json::from_str(body(&message)).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn frames_with_the_octet_count() {
        let mut event = fixture::event();
        event.user_agent = Some("caf\u{e9} \u{1F600}".to_owned());
        let first = message(&event, &HEADER, None);
        let second = message(&fixture::bare(), &HEADER, None);
        let mut stream = octet_counted(&first);
        stream.extend(octet_counted(&second));
        assert!(stream.starts_with(format!("{} <", first.len()).as_bytes()));

        let mut frames = Vec::new();
        let mut rest = stream.as_slice();
        while !rest.is_empty() {
            let space = rest.iter().position(|byte| *byte == b' ').unwrap();
            let length: usize = std::str::from_utf8(&rest[..space])
                .unwrap()
                .parse()
                .unwrap();
            assert_ne!(rest[0], b'0');
            frames.push(std::str::from_utf8(&rest[space + 1..space + 1 + length]).unwrap());
            rest = &rest[space + 1 + length..];
        }
        assert_eq!(frames, [first.as_str(), second.as_str()]);
    }

    #[test]
    fn leaves_messages_within_the_budget_whole() {
        let event = fixture::event();
        let whole = message(&event, &HEADER, None);
        assert_eq!(message(&event, &HEADER, Some(UDP_BUDGET)), whole);
        assert_eq!(message(&event, &HEADER, Some(whole.len())), whole);
    }

    #[test]
    fn truncates_to_the_udp_budget_on_a_character_boundary() {
        let mut event = fixture::event();
        event.user_agent = Some("\u{e9}\u{1F600}".repeat(UDP_BUDGET / 4));
        let whole = message(&event, &HEADER, None);
        assert!(whole.len() > UDP_BUDGET);

        let truncated = message(&event, &HEADER, Some(UDP_BUDGET));
        assert!(truncated.len() <= UDP_BUDGET);
        assert!(truncated.len() > UDP_BUDGET - 4);
        assert!(whole.starts_with(&truncated));
        assert!(truncated.contains(BOM));
    }

    #[test]
    fn truncation_never_splits_a_character() {
        let mut event = fixture::event();
        event.user_agent = Some("\u{1F600}".repeat(16));
        let whole = message(&event, &HEADER, None);
        let start = whole.find('\u{1F600}').unwrap();
        for budget in start..start + 8 {
            let truncated = message(&event, &HEADER, Some(budget));
            assert!(truncated.len() <= budget);
            assert!(truncated.len() > budget.saturating_sub(4));
            assert!(whole.starts_with(&truncated));
        }
    }
}
