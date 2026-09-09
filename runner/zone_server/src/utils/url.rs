//! URL validation for outbound HTTP fetches.

use std::net::IpAddr;

/// Parse `raw` and reject schemes, hosts, and addresses that must not be fetched
/// by server-side knowledge or tool requests.
pub fn validate_public_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "Invalid URL.".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only http and https URLs are allowed.".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URLs with embedded credentials are not allowed.".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "URL must have a host.".to_string())?;

    // `host_str` hands back IPv6 literals still bracketed, and `[::1]` does not
    // parse as an address -- so without this every IPv6 spelling of a private
    // target walked straight past the check below.
    let literal = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);

    if let Ok(ip) = literal.parse::<IpAddr>() {
        if is_private_ip(ip) {
            return Err("Private IP addresses are not allowed.".to_string());
        }
        return Ok(url);
    }

    // A trailing dot is the same name to a resolver and a different string to
    // `==`, so `localhost.` reached loopback while `localhost` did not.
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host == "metadata.google.internal"
    {
        return Err("Internal hostnames are not allowed.".to_string());
    }
    Ok(url)
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.octets()[0] == 0
                || ip.octets() == [169, 254, 169, 254]
        }
        // An IPv4 address written as IPv6 reaches the same host, so it is
        // answered by the IPv4 rules rather than a second, weaker set.
        IpAddr::V6(ip) => match ip.to_ipv4_mapped().or_else(|| ip.to_ipv4()) {
            Some(ip) => is_private_ip(IpAddr::V4(ip)),
            None => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_private_and_internal_targets() {
        for url in [
            "http://127.0.0.1/",
            "http://10.0.0.1/",
            "http://localhost/admin",
            "http://169.254.169.254/latest",
            "file:///etc/passwd",
            "https://user:pass@example.com/",
        ] {
            assert!(validate_public_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn rejects_ipv6_and_trailing_dot_spellings_of_the_same_targets() {
        for url in [
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[::ffff:169.254.169.254]/latest/meta-data/",
            "http://[fd00::1]/",
            "http://[fe80::1]/",
            "http://localhost./",
            "http://LOCALHOST/",
            "http://127.0.0.1./",
        ] {
            assert!(validate_public_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn accepts_public_https() {
        assert_eq!(
            validate_public_url("https://example.com/docs")
                .unwrap()
                .as_str(),
            "https://example.com/docs"
        );
    }
}
