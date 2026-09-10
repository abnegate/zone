//! URL validation for outbound HTTP fetches.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// Redirect hops a caller-supplied fetch may follow. Each one is validated, so
/// this bounds the chain rather than the trust.
const MAX_REDIRECTS: usize = 3;

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
        if must_not_be_fetched(ip) {
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

/// A client builder for fetching a caller-supplied URL.
///
/// [`validate_public_url`] only answers for the URL it was handed. reqwest
/// issues the redirect hops itself, so the policy re-checks every hop: reading
/// the final address back afterwards refuses the disclosure but has already
/// made the request.
/// Resolves a name and refuses the addresses `validate_public_url` cannot see.
///
/// The URL check reads text; the address a name resolves to is chosen by DNS
/// afterwards, so a public hostname pointing at 127.0.0.1 or a LAN address
/// passes every textual check and is still an internal request. Refusing at
/// the connector is what makes the guard hold, because every request and every
/// redirect hop must resolve before it can connect.
struct PublicAddresses;

impl Resolve for PublicAddresses {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let resolved = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let public = public_only(resolved, &host)?;
            Ok(Box::new(public.into_iter()) as Addrs)
        })
    }
}

/// Keep only the addresses a caller-supplied fetch may connect to.
///
/// A name that answers with both a public and a private address is not
/// refused outright -- the private one is dropped, so a connector that falls
/// back through the list cannot arrive at it.
fn public_only(
    addresses: impl Iterator<Item = SocketAddr>,
    host: &str,
) -> Result<Vec<SocketAddr>, String> {
    let public: Vec<SocketAddr> = addresses
        .filter(|address| !must_not_be_fetched(address.ip()))
        .collect();

    if public.is_empty() {
        return Err(format!(
            "{host} resolves only to addresses that must not be fetched."
        ));
    }

    Ok(public)
}

pub fn public_client_builder(timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .timeout(timeout)
        .dns_resolver(std::sync::Arc::new(PublicAddresses))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > MAX_REDIRECTS {
                attempt.error("Too many redirects.".to_string())
            } else {
                match validate_public_url(attempt.url().as_str()) {
                    Ok(_) => attempt.follow(),
                    Err(error) => attempt.error(error),
                }
            }
        }))
}

pub fn public_client(timeout: Duration) -> Result<reqwest::Client, String> {
    public_client_builder(timeout)
        .build()
        .map_err(|error| error.to_string())
}

/// Read at most `limit` bytes of `response`, refusing a body that does not fit
/// rather than buffering it whole and measuring it afterwards.
pub async fn read_capped(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, String> {
    let oversized = || format!("The response body is larger than {limit} bytes.");
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(oversized());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Could not read the response body.".to_string())?
    {
        if body.len() + chunk.len() > limit {
            return Err(oversized());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Whether `ip` is an address a caller-supplied fetch must not reach.
///
/// `Ipv4Addr::is_global` would answer this, but it is still unstable, so the
/// non-global ranges are named here. Enumerating them is the whole point: the
/// obvious three private blocks leave shared address space (`100.64.0.0/10`,
/// which a carrier or cloud network routes internally) and benchmarking space
/// reachable, and those are internal destinations like any other.
fn must_not_be_fetched(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_broadcast()
                || octets[0] == 0
                || octets[0] >= 240
                // Shared address space, which carrier and cloud networks route.
                || (octets[0] == 100 && (64..128).contains(&octets[1]))
                // Benchmarking.
                || (octets[0] == 198 && (18..20).contains(&octets[1]))
                // IETF protocol assignments, 6to4 relay anycast, and the three
                // documentation ranges.
                || matches!(
                    [octets[0], octets[1], octets[2]],
                    [192, 0, 0] | [192, 0, 2] | [192, 88, 99] | [198, 51, 100] | [203, 0, 113]
                )
        }
        // An IPv4 address written as IPv6 reaches the same host, so it is
        // answered by the IPv4 rules rather than a second, weaker set.
        IpAddr::V6(ip) => match ip.to_ipv4_mapped().or_else(|| ip.to_ipv4()) {
            Some(ip) => must_not_be_fetched(IpAddr::V4(ip)),
            None => {
                let segments = ip.segments();
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
                    || ip.is_multicast()
                    // Discard-only.
                    || (segments[0] == 0x0100 && segments[1..4] == [0, 0, 0])
                    // IETF protocol assignments, Teredo among them.
                    || (segments[0] == 0x2001 && segments[1] < 0x0200)
                    // Documentation.
                    || (segments[0] == 0x2001 && segments[1] == 0x0db8)
                    || (segments[0] & 0xfff0) == 0x3ff0
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(raw: &str) -> SocketAddr {
        raw.parse().expect("a socket address")
    }

    /// The three private blocks are not the whole of what is unreachable from
    /// outside. Shared address space is routed inside carrier and cloud
    /// networks, and the rest of these are addresses no public name should
    /// ever answer with.
    #[test]
    fn the_non_global_ranges_beyond_the_private_ones_are_refused() {
        for raw in [
            "100.64.0.1",      // shared address space
            "100.127.255.1",   // shared address space, upper edge
            "198.18.0.1",      // benchmarking
            "198.19.255.1",    // benchmarking, upper edge
            "192.0.0.1",       // IETF protocol assignments
            "192.0.2.1",       // documentation
            "198.51.100.1",    // documentation
            "203.0.113.1",     // documentation
            "192.88.99.1",     // 6to4 relay anycast
            "224.0.0.1",       // multicast
            "240.0.0.1",       // reserved
            "255.255.255.255", // broadcast
        ] {
            let ip: IpAddr = raw.parse().expect("an address");
            assert!(must_not_be_fetched(ip), "{raw} is reachable");
            assert!(
                validate_public_url(&format!("http://{raw}/")).is_err(),
                "{raw} passed the URL check"
            );
        }
    }

    #[test]
    fn a_globally_routable_address_is_still_reachable() {
        for raw in [
            "93.184.216.34",
            "1.1.1.1",
            "100.63.255.255",
            "198.17.255.255",
        ] {
            let ip: IpAddr = raw.parse().expect("an address");
            assert!(!must_not_be_fetched(ip), "{raw} was refused");
        }
    }

    /// `validate_public_url` reads text. Which address a name answers with is
    /// chosen afterwards by DNS, so a public hostname pointing at loopback
    /// passes every textual check and is still an internal request.
    #[test]
    fn a_name_answering_only_with_private_addresses_is_refused() {
        let error = public_only(
            [address("127.0.0.1:0"), address("[::1]:0")].into_iter(),
            "inside.example",
        )
        .expect_err("loopback must not be fetched");
        assert!(error.contains("inside.example"), "{error}");
    }

    #[test]
    fn a_name_answering_with_a_public_address_is_allowed() {
        let public = public_only([address("93.184.216.34:0")].into_iter(), "example.test")
            .expect("a public address is fetchable");
        assert_eq!(public, vec![address("93.184.216.34:0")]);
    }

    /// The private address is dropped rather than the whole answer refused, so
    /// a connector working down the list cannot fall back onto it.
    #[test]
    fn a_private_address_beside_a_public_one_is_dropped() {
        let public = public_only(
            [address("127.0.0.1:0"), address("93.184.216.34:0")].into_iter(),
            "both.example",
        )
        .expect("the public address stands");
        assert_eq!(
            public,
            vec![address("93.184.216.34:0")],
            "the loopback address survived alongside the public one"
        );
    }

    #[tokio::test]
    async fn the_client_resolver_refuses_a_name_that_answers_with_loopback() {
        let name: Name = "localhost".parse().expect("a resolvable name");
        let error = Resolve::resolve(&PublicAddresses, name)
            .await
            .err()
            .expect("localhost must not be fetchable");
        assert!(error.to_string().contains("must not be fetched"), "{error}");
    }

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
