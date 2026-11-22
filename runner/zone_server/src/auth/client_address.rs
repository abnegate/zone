#[cfg(test)]
mod tests {
    use std::net::{IpAddr, SocketAddr};

    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{HeaderMap, HeaderValue, Request};

    use super::*;
    use crate::config::TrustedProxies;
    use crate::state::AppState;

    const LOOPBACK: &str = "127.0.0.1";

    fn address(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn forwarded(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(FORWARDED_FOR, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    fn resolved(peer: &str, values: &[&str]) -> IpAddr {
        resolve(
            address(peer),
            &forwarded(values),
            &TrustedProxies::default(),
        )
    }

    fn edge() -> TrustedProxies {
        TrustedProxies::parse(Some("127.0.0.0/8,172.31.0.0/24")).unwrap()
    }

    #[test]
    fn an_untrusted_peer_is_the_client_whatever_it_forwards() {
        for peer in ["203.0.113.9", "192.168.1.5", "10.0.0.2"] {
            assert_eq!(resolved(peer, &["198.51.100.7"]), address(peer), "{peer}");
            assert_eq!(resolved(peer, &[LOOPBACK]), address(peer), "{peer}");
        }
    }

    #[test]
    fn a_trusted_peer_without_a_forwarded_header_is_the_client() {
        assert_eq!(resolved(LOOPBACK, &[]), address(LOOPBACK));
    }

    #[test]
    fn behind_a_trusted_peer_the_rightmost_untrusted_hop_is_the_client() {
        assert_eq!(resolved(LOOPBACK, &["198.51.100.7"]), address("198.51.100.7"));
        assert_eq!(
            resolve(
                address("172.31.0.3"),
                &forwarded(&["198.51.100.7, 172.31.0.9, 127.0.0.1"]),
                &edge(),
            ),
            address("198.51.100.7")
        );
    }

    #[test]
    fn a_spoofed_leftmost_entry_behind_a_trusted_proxy_is_not_the_client() {
        assert_eq!(
            resolved(LOOPBACK, &["1.2.3.4, 198.51.100.7"]),
            address("198.51.100.7")
        );
        assert_eq!(
            resolved(LOOPBACK, &["127.0.0.1, 198.51.100.7"]),
            address("198.51.100.7")
        );
    }

    #[test]
    fn every_header_line_is_read_in_order() {
        assert_eq!(
            resolved(LOOPBACK, &["1.2.3.4", "198.51.100.7"]),
            address("198.51.100.7")
        );
        assert_eq!(
            resolve(
                address("172.31.0.3"),
                &forwarded(&["198.51.100.7, 172.31.0.8", "172.31.0.9"]),
                &edge(),
            ),
            address("198.51.100.7")
        );
    }

    #[test]
    fn when_every_hop_is_trusted_the_leftmost_is_the_client() {
        assert_eq!(
            resolve(
                address("172.31.0.3"),
                &forwarded(&["127.0.0.5, 172.31.0.9"]),
                &edge(),
            ),
            address("127.0.0.5")
        );
    }

    #[test]
    fn an_unparsable_entry_stops_at_the_last_trusted_hop() {
        for garbage in [
            "unknown",
            "198.51.100.7:4711",
            "[2001:db8::1]:443",
            "",
            "_hidden",
            "198.51.100.7 extra",
        ] {
            assert_eq!(
                resolved(LOOPBACK, &[&format!("198.51.100.7, {garbage}")]),
                address(LOOPBACK),
                "{garbage:?}"
            );
        }
        assert_eq!(
            resolve(
                address("172.31.0.3"),
                &forwarded(&["198.51.100.7, garbage, 172.31.0.9"]),
                &edge(),
            ),
            address("172.31.0.9")
        );
    }

    #[test]
    fn a_header_value_that_is_not_text_is_unparsable() {
        let mut headers = HeaderMap::new();
        headers.append(FORWARDED_FOR, HeaderValue::from_static("198.51.100.7"));
        headers.append(FORWARDED_FOR, HeaderValue::from_bytes(b"\xff\xfe").unwrap());

        assert_eq!(
            resolve(address(LOOPBACK), &headers, &TrustedProxies::default()),
            address(LOOPBACK)
        );
    }

    #[test]
    fn the_real_ip_header_is_never_read() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.7"));

        for peer in [LOOPBACK, "203.0.113.9"] {
            assert_eq!(
                resolve(address(peer), &headers, &TrustedProxies::default()),
                address(peer)
            );
        }
    }

    #[test]
    fn an_ipv4_mapped_peer_or_hop_reads_as_ipv4() {
        assert_eq!(
            resolved("::ffff:127.0.0.1", &["::ffff:198.51.100.7"]),
            address("198.51.100.7")
        );
        assert_eq!(resolved("::ffff:203.0.113.9", &[]), address("203.0.113.9"));
    }

    #[test]
    fn an_ipv6_client_is_keyed_by_its_network() {
        assert_eq!(key(address("2001:db8:1:2:aaaa::1")), address("2001:db8:1:2::"));
        assert_eq!(
            key(address("2001:db8:1:2:ffff:ffff:ffff:ffff")),
            key(address("2001:db8:1:2::9"))
        );
        assert_ne!(key(address("2001:db8:1:2::1")), key(address("2001:db8:1:3::1")));
    }

    #[test]
    fn an_ipv4_client_is_keyed_as_itself() {
        assert_eq!(key(address("198.51.100.7")), address("198.51.100.7"));
        assert_eq!(key(address("::ffff:198.51.100.7")), address("198.51.100.7"));
        assert_ne!(key(address("198.51.100.7")), key(address("198.51.100.8")));
    }

    #[test]
    fn a_connected_request_resolves_through_the_trusted_proxies() {
        let request = Request::builder()
            .header(FORWARDED_FOR, "198.51.100.7")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))))
            .body(Body::empty())
            .unwrap();
        let (parts, _) = request.into_parts();

        assert_eq!(
            ClientAddress::of(&parts, &TrustedProxies::default()),
            ClientAddress(Some(address("198.51.100.7")))
        );
        assert_eq!(
            ClientAddress::of(&parts, &TrustedProxies::parse(Some("")).unwrap()),
            ClientAddress(Some(address(LOOPBACK)))
        );
    }

    #[tokio::test]
    async fn the_extractor_finds_no_address_without_a_connection() {
        let request = Request::builder()
            .header(FORWARDED_FOR, "198.51.100.7")
            .body(Body::empty())
            .unwrap();
        let (mut parts, _) = request.into_parts();

        let extracted = ClientAddress::from_request_parts(&mut parts, &AppState::for_tests())
            .await
            .unwrap();

        assert_eq!(extracted, ClientAddress(None));
    }
}
