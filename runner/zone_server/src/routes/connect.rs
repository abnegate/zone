//! Origins a phone can type into the Zone app.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header::HOST},
};
use serde::Serialize;

use crate::auth::AuthUser;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ConnectResponse {
    urls: Vec<String>,
}

/// GET /api/connect
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    headers: HeaderMap,
) -> Json<ConnectResponse> {
    Json(ConnectResponse {
        urls: advertised_urls(&state.config().connect_urls, &headers),
    })
}

pub(crate) fn advertised_urls(configured: &[String], headers: &HeaderMap) -> Vec<String> {
    let mut urls = Vec::new();
    for url in configured {
        push_unique(&mut urls, url.clone());
    }
    if let Some(origin) = request_origin(headers) {
        push_unique(&mut urls, origin);
    }
    urls
}

fn request_origin(headers: &HeaderMap) -> Option<String> {
    let host = header(headers, "x-forwarded-host").or_else(|| header(headers, HOST.as_str()))?;
    let scheme = header(headers, "x-forwarded-proto").unwrap_or("http");
    phone_reachable_origin(scheme, host)
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// An origin a phone on this network can open: a non-loopback IPv4 host or a
/// Bonjour `*.local` name. `manager.localhost` stays on this computer.
pub(crate) fn phone_reachable_origin(scheme: &str, host: &str) -> Option<String> {
    let host = host.trim();
    if host.is_empty() {
        return None;
    }
    let candidate = format!("{scheme}://{host}");
    let url = reqwest::Url::parse(&candidate).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let hostname = url.host_str()?;
    if !phone_reachable_host(hostname) {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

pub(crate) fn phone_reachable_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return false;
    }
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        return address.is_ipv4() && !address.is_loopback() && !address.is_unspecified();
    }
    host.ends_with(".local")
}

fn push_unique(urls: &mut Vec<String>, url: String) {
    if !urls.iter().any(|existing| existing == &url) {
        urls.push(url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn lan_ip_and_bonjour_are_phone_reachable() {
        assert!(phone_reachable_host("192.168.1.10"));
        assert!(phone_reachable_host("10.0.2.2"));
        assert!(phone_reachable_host("100.64.1.2"));
        assert!(phone_reachable_host("jake-macbook.local"));
        assert!(phone_reachable_host("JAKE-MACBOOK.LOCAL"));
        assert!(!phone_reachable_host("127.0.0.1"));
        assert!(!phone_reachable_host("0.0.0.0"));
        assert!(!phone_reachable_host("localhost"));
        assert!(!phone_reachable_host("manager.localhost"));
        assert!(!phone_reachable_host("manager.webui.localhost"));
        assert!(!phone_reachable_host("zone.example.com"));
    }

    #[test]
    fn request_origin_prefers_forwarded_lan_host() {
        let headers = headers(&[
            ("x-forwarded-proto", "http"),
            ("x-forwarded-host", "192.168.1.10"),
            ("host", "manager.localhost"),
        ]);
        assert_eq!(advertised_urls(&[], &headers), ["http://192.168.1.10"]);
    }

    #[test]
    fn configured_urls_come_first_and_the_request_origin_is_not_repeated() {
        let headers = headers(&[("host", "192.168.1.10")]);
        assert_eq!(
            advertised_urls(
                &["http://192.168.1.10".into(), "http://100.64.1.2".into()],
                &headers
            ),
            ["http://192.168.1.10", "http://100.64.1.2"]
        );
    }

    #[test]
    fn manager_localhost_is_not_advertised() {
        let headers = headers(&[("host", "manager.localhost")]);
        assert_eq!(
            advertised_urls(&["http://192.168.1.10".into()], &headers),
            ["http://192.168.1.10"]
        );
        assert!(advertised_urls(&[], &headers).is_empty());
    }
}
