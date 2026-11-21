use std::net::IpAddr;

const MAXIMUM_LENGTH: usize = 253;
const MAXIMUM_LABEL_LENGTH: usize = 63;
const SEPARATOR: char = '.';
const HYPHEN: char = '-';
const WILDCARD_PREFIX: &str = "*.";
const OPENING_BRACKET: char = '[';
const CLOSING_BRACKET: char = ']';

pub(super) fn normalized(entry: &str) -> Option<String> {
    let lowered = entry.trim().to_ascii_lowercase();
    let host = lowered.strip_prefix(WILDCARD_PREFIX).unwrap_or(&lowered);
    canonical(host)
}

pub(super) fn matches(entry: &str, host: &str) -> bool {
    canonical(&host.trim().to_ascii_lowercase()).is_some_and(|host| covers(entry, &host))
}

pub(super) fn covers(outer: &str, inner: &str) -> bool {
    inner == outer
        || (!literal(outer)
            && !literal(inner)
            && inner
                .strip_suffix(outer)
                .is_some_and(|subdomain| subdomain.ends_with(SEPARATOR)))
}

fn literal(host: &str) -> bool {
    host.parse::<IpAddr>().is_ok()
}

fn canonical(host: &str) -> Option<String> {
    let host = host.strip_suffix(SEPARATOR).unwrap_or(host);
    let unbracketed = host
        .strip_prefix(OPENING_BRACKET)
        .and_then(|inner| inner.strip_suffix(CLOSING_BRACKET));
    if let Some(address) = unbracketed {
        return match address.parse::<IpAddr>() {
            Ok(IpAddr::V6(address)) => Some(address.to_string()),
            _ => None,
        };
    }
    if let Ok(address) = host.parse::<IpAddr>() {
        return Some(address.to_string());
    }
    hostname(host).then(|| host.to_owned())
}

fn hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= MAXIMUM_LENGTH
        && host.split(SEPARATOR).all(label)
        && !host
            .rsplit(SEPARATOR)
            .next()
            .is_some_and(|top| top.chars().all(|character| character.is_ascii_digit()))
}

fn label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAXIMUM_LABEL_LENGTH
        && !label.starts_with(HYPHEN)
        && !label.ends_with(HYPHEN)
        && label.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == HYPHEN
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_and_strips_wildcards_and_trailing_dots() {
        for (entry, expected) in [
            (" Example.COM ", "example.com"),
            ("*.example.com", "example.com"),
            ("example.com.", "example.com"),
            ("*.Example.com.", "example.com"),
            ("localhost", "localhost"),
            ("api-1.eu.example.co.uk", "api-1.eu.example.co.uk"),
        ] {
            assert_eq!(normalized(entry).as_deref(), Some(expected), "{entry}");
        }
    }

    #[test]
    fn accepts_ip_literals_in_canonical_form() {
        for (entry, expected) in [
            ("10.0.0.1", "10.0.0.1"),
            ("::1", "::1"),
            ("[::1]", "::1"),
            ("[2001:DB8:0:0::1]", "2001:db8::1"),
        ] {
            assert_eq!(normalized(entry).as_deref(), Some(expected), "{entry}");
        }
    }

    #[test]
    fn refuses_schemes_ports_paths_and_malformed_hosts() {
        for entry in [
            "",
            " ",
            "*",
            "*.",
            "https://example.com",
            "example.com:443",
            "example.com/path",
            "user@example.com",
            "*.*.example.com",
            "api.*.example.com",
            "example..com",
            ".example.com",
            "-example.com",
            "example-.com",
            "exa_mple.com",
            "exa mple.com",
            "bücher.de",
            "1.2.3",
            "10.0.0.256",
            "[10.0.0.1]",
            "[::1",
            &format!("{}.com", "a".repeat(MAXIMUM_LABEL_LENGTH + 1)),
            &format!("{}a", "a.".repeat(MAXIMUM_LENGTH / 2 + 1)),
        ] {
            assert_eq!(normalized(entry), None, "{entry:?}");
        }
    }

    #[test]
    fn a_domain_matches_itself_and_every_subdomain() {
        assert!(matches("example.com", "example.com"));
        assert!(matches("example.com", "API.Example.com."));
        assert!(matches("example.com", "a.b.example.com"));
        assert!(!matches("example.com", "badexample.com"));
        assert!(!matches("example.com", "example.com.evil.test"));
        assert!(!matches("example.com", "com"));
        assert!(!matches("example.com", ""));
        assert!(!matches("example.com", "example.com:443"));
    }

    #[test]
    fn an_ip_literal_matches_only_itself() {
        assert!(matches("10.0.0.1", "10.0.0.1"));
        assert!(matches("::1", "[::1]"));
        assert!(matches("2001:db8::1", "[2001:db8:0::1]"));
        assert!(!matches("10.0.0.1", "10.0.0.10"));
        assert!(!matches("0.0.1", "10.0.0.1"));
    }

    #[test]
    fn a_domain_covers_its_subdomains_only() {
        assert!(covers("example.com", "example.com"));
        assert!(covers("example.com", "eu.example.com"));
        assert!(!covers("eu.example.com", "example.com"));
        assert!(!covers("example.com", "myexample.com"));
        assert!(!covers("0.0.1", "10.0.0.1"));
    }
}
