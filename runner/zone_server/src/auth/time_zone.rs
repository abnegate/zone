//! The time zone a console reports on login, registration and refresh.

use axum::http::HeaderMap;

use crate::services::time_zone::TimeZone;

pub const HEADER: &str = "x-zone-time-zone";

/// The reported zone, or `None` when none was sent or it names no IANA zone.
pub fn claim(headers: &HeaderMap) -> Option<TimeZone> {
    headers
        .get(HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|name| TimeZone::parse(name).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn a_reported_zone_is_read_only_when_it_is_an_iana_name() {
        let mut headers = HeaderMap::new();
        assert_eq!(claim(&headers), None);

        headers.insert(HEADER, HeaderValue::from_static("Pacific/Auckland"));
        assert_eq!(
            claim(&headers).map(TimeZone::name),
            Some("Pacific/Auckland")
        );

        headers.insert(HEADER, HeaderValue::from_static("Middle/Earth"));
        assert_eq!(claim(&headers), None);
    }
}
