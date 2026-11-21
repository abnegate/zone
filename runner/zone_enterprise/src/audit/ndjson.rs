use super::Event;

const SEPARATOR: u8 = b'\n';

pub fn line(event: &Event) -> serde_json::Result<Vec<u8>> {
    let mut line = serde_json::to_vec(event)?;
    line.push(SEPARATOR);
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::event::fixture;

    #[test]
    fn writes_one_event_per_line() {
        let line = line(&fixture::event()).unwrap();
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(line.iter().filter(|byte| **byte == b'\n').count(), 1);
        let parsed: Event = serde_json::from_slice(&line[..line.len() - 1]).unwrap();
        assert_eq!(parsed, fixture::event());
    }

    #[test]
    fn escapes_line_breaks_inside_values() {
        let mut event = fixture::event();
        event.user_agent = Some("first\nsecond\r\nthird\u{2028}".to_owned());
        let line = line(&event).unwrap();
        assert_eq!(line.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert!(!line.contains(&b'\r'));
    }

    #[test]
    fn concatenated_lines_parse_back_in_order() {
        let mut second = fixture::bare();
        second.action = "user.login".to_owned();
        let mut body = line(&fixture::event()).unwrap();
        body.extend(line(&second).unwrap());
        let parsed: Vec<Event> = serde_json::Deserializer::from_slice(&body)
            .into_iter()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(parsed, [fixture::event(), second]);
        assert_eq!(std::str::from_utf8(&body).unwrap().lines().count(), 2);
    }
}
