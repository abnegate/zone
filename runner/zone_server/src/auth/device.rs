//! Device identity carried on login, refresh, and authenticated requests.

use axum::http::HeaderMap;
use uuid::Uuid;

use crate::db::devices::{self, Claim, Platform};

pub const PUBLIC_ID: &str = "x-zone-device";
pub const NAME: &str = "x-zone-device-name";
pub const PLATFORM: &str = "x-zone-device-platform";

pub const PENDING: &str = "device_pending";
pub const BLOCKED: &str = "device_blocked";

pub fn claim(headers: &HeaderMap) -> Claim {
    Claim {
        public_id: header(headers, PUBLIC_ID).and_then(|value| Uuid::parse_str(value).ok()),
        name: header(headers, NAME).map(str::to_string),
        platform: header(headers, PLATFORM).map(Platform::parse),
        user_agent: header(headers, "user-agent").map(str::to_string),
        ip_address: client_ip(headers),
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn client_ip(headers: &HeaderMap) -> Option<String> {
    header(headers, "x-forwarded-for")
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| header(headers, "x-real-ip").map(str::to_string))
        .filter(|value| value.parse::<std::net::IpAddr>().is_ok())
}

pub fn info(claim: &Claim, device: &devices::Device) -> serde_json::Value {
    let name = device
        .name
        .as_deref()
        .or(claim.name.as_deref())
        .unwrap_or(device.platform.as_str());
    serde_json::json!({
        "platform": device.platform.as_str(),
        "name": name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn reads_a_device_claim_from_headers() {
        let mut headers = HeaderMap::new();
        let id = Uuid::new_v4();
        headers.insert(PUBLIC_ID, HeaderValue::from_str(&id.to_string()).unwrap());
        headers.insert(PLATFORM, HeaderValue::from_static("android"));
        headers.insert(NAME, HeaderValue::from_static("S23"));
        headers.insert("user-agent", HeaderValue::from_static("Zone/1"));
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("192.168.4.31, 172.30.0.1"),
        );

        let claim = claim(&headers);
        assert_eq!(claim.public_id, Some(id));
        assert_eq!(claim.platform, Some(Platform::Android));
        assert_eq!(claim.name.as_deref(), Some("S23"));
        assert_eq!(claim.user_agent.as_deref(), Some("Zone/1"));
        assert_eq!(claim.ip_address.as_deref(), Some("192.168.4.31"));
    }

    #[test]
    fn a_missing_platform_header_does_not_claim_browser() {
        let claim = claim(&HeaderMap::new());
        assert_eq!(claim.platform, None);
        assert_eq!(claim.public_id, None);
    }
}
