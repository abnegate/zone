#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;

    fn address(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn unset_trusts_loopback_only() {
        let trusted = TrustedProxies::parse(None).unwrap();

        assert_eq!(trusted, TrustedProxies::default());
        for loopback in ["127.0.0.1", "127.255.0.9", "::1", "::ffff:127.0.0.1"] {
            assert!(trusted.trusts(address(loopback)), "{loopback}");
        }
        for untrusted in ["192.168.1.5", "10.0.0.1", "172.31.0.2", "203.0.113.9", "::2"] {
            assert!(!trusted.trusts(address(untrusted)), "{untrusted}");
        }
    }

    #[test]
    fn a_list_replaces_the_default() {
        let trusted =
            TrustedProxies::parse(Some(" 172.31.0.0/24, 10.1.2.3 ,fc00::/7,,")).unwrap();

        for listed in ["172.31.0.1", "172.31.0.254", "10.1.2.3", "fd12::1"] {
            assert!(trusted.trusts(address(listed)), "{listed}");
        }
        for unlisted in ["127.0.0.1", "::1", "172.31.1.1", "10.1.2.4"] {
            assert!(!trusted.trusts(address(unlisted)), "{unlisted}");
        }
    }

    #[test]
    fn an_empty_list_trusts_nothing() {
        for empty in ["", " ", " , "] {
            let trusted = TrustedProxies::parse(Some(empty)).unwrap();
            assert!(!trusted.trusts(address("127.0.0.1")), "{empty:?}");
            assert!(!trusted.trusts(address("::1")), "{empty:?}");
        }
    }

    #[test]
    fn an_invalid_entry_is_refused_by_name() {
        for invalid in ["proxy.internal", "10.0.0.0/33", "10.0.0.1:8080", "*"] {
            let refused = TrustedProxies::parse(Some(&format!("127.0.0.1, {invalid}"))).unwrap_err();

            assert!(
                matches!(&refused, ConfigError::TrustedProxy(entry) if entry == invalid),
                "{invalid}: {refused}"
            );
            assert!(refused.to_string().contains(invalid), "{refused}");
            assert!(refused.to_string().contains(VARIABLE), "{refused}");
        }
    }
}
