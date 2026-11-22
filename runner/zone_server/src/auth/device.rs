//! Device identity carried on login, refresh, and authenticated requests.

use std::net::IpAddr;

use axum::http::HeaderMap;
use axum::http::header::USER_AGENT;
use uuid::Uuid;

use crate::db::devices::{self, Claim, Platform};

pub const PUBLIC_ID: &str = "x-zone-device";
pub const NAME: &str = "x-zone-device-name";
pub const PLATFORM: &str = "x-zone-device-platform";

pub const PENDING: &str = "device_pending";
pub const BLOCKED: &str = "device_blocked";

/// `address` is the resolved client address; forwarding headers are never read here.
pub fn claim(headers: &HeaderMap, address: Option<IpAddr>) -> Claim {
    Claim {
        public_id: header(headers, PUBLIC_ID).and_then(|value| Uuid::parse_str(value).ok()),
        name: header(headers, NAME).map(str::to_string),
        platform: header(headers, PLATFORM).map(Platform::parse),
        user_agent: header(headers, USER_AGENT.as_str()).map(str::to_string),
        ip_address: address.map(|address| address.to_string()),
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
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
    use std::net::SocketAddr;

    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{HeaderValue, Request};

    use super::*;
    use crate::auth::client_address::{ClientAddress, FORWARDED_FOR};
    use crate::config::TrustedProxies;

    #[test]
    fn reads_a_device_claim_from_headers() {
        let mut headers = HeaderMap::new();
        let id = Uuid::new_v4();
        headers.insert(PUBLIC_ID, HeaderValue::from_str(&id.to_string()).unwrap());
        headers.insert(PLATFORM, HeaderValue::from_static("android"));
        headers.insert(NAME, HeaderValue::from_static("S23"));
        headers.insert("user-agent", HeaderValue::from_static("Zone/1"));

        let claim = claim(&headers, Some(IpAddr::from([192, 168, 4, 31])));
        assert_eq!(claim.public_id, Some(id));
        assert_eq!(claim.platform, Some(Platform::Android));
        assert_eq!(claim.name.as_deref(), Some("S23"));
        assert_eq!(claim.user_agent.as_deref(), Some("Zone/1"));
        assert_eq!(claim.ip_address.as_deref(), Some("192.168.4.31"));
    }

    #[test]
    fn the_claim_records_the_address_it_is_given_and_never_a_forwarding_header() {
        let mut headers = HeaderMap::new();
        headers.insert(FORWARDED_FOR, HeaderValue::from_static("198.51.100.7"));
        headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.8"));

        assert_eq!(
            claim(&headers, Some(IpAddr::from([203, 0, 113, 9])))
                .ip_address
                .as_deref(),
            Some("203.0.113.9")
        );
        assert_eq!(claim(&headers, None).ip_address, None);
    }

    #[test]
    fn a_spoofed_forwarded_address_from_an_untrusted_peer_records_the_peer() {
        let request = Request::builder()
            .header(FORWARDED_FOR, "192.168.4.31")
            .header("x-real-ip", "192.168.4.32")
            .extension(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50_000))))
            .body(Body::empty())
            .unwrap();
        let (parts, _) = request.into_parts();
        let ClientAddress(address) = ClientAddress::of(&parts, &TrustedProxies::default());

        assert_eq!(
            claim(&parts.headers, address).ip_address.as_deref(),
            Some("203.0.113.9")
        );
    }

    #[test]
    fn a_missing_platform_header_does_not_claim_browser() {
        let claim = claim(&HeaderMap::new(), None);
        assert_eq!(claim.platform, None);
        assert_eq!(claim.public_id, None);
    }
}
