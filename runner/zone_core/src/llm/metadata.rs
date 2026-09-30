//! The link-local and cloud metadata targets a tenant's endpoint must never
//! reach. LAN, loopback and single-label hosts stay reachable: a self-hosted
//! Zone runs its models on them.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::{Attempt, Policy};
use reqwest::{Url, redirect};

const HOSTS: [&str; 2] = ["metadata.google.internal", "metadata.goog"];
const IPV4_ADDRESSES: [Ipv4Addr; 1] = [Ipv4Addr::new(100, 100, 100, 200)];
const IPV6_ADDRESSES: [Ipv6Addr; 1] = [Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254)];
const REDIRECTS: usize = 10;

/// Whether `address` is link-local or a cloud provider's metadata service.
pub fn is_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => address.is_link_local() || IPV4_ADDRESSES.contains(&address),
        IpAddr::V6(address) => match address.to_ipv4_mapped() {
            Some(mapped) => is_address(IpAddr::V4(mapped)),
            None => address.is_unicast_link_local() || IPV6_ADDRESSES.contains(&address),
        },
    }
}

/// Whether `host` names a cloud provider's metadata service, or is a
/// link-local or metadata address written as one.
pub fn is_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let literal = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(&host);
    if let Ok(address) = literal.parse::<IpAddr>() {
        return is_address(address);
    }
    HOSTS.contains(&literal)
}

/// Whether `url` points at a host [`is_host`] refuses.
pub fn is_url(url: &Url) -> bool {
    url.host_str().is_some_and(is_host)
}

/// `addresses` without the ones [`is_address`] refuses, or an error when
/// nothing else is left to connect to.
fn kept(
    addresses: impl Iterator<Item = SocketAddr>,
    host: &str,
) -> Result<Vec<SocketAddr>, String> {
    let kept: Vec<SocketAddr> = addresses
        .filter(|address| !is_address(address.ip()))
        .collect();
    if kept.is_empty() {
        return Err(format!(
            "{host} resolves only to link-local or cloud metadata addresses"
        ));
    }
    Ok(kept)
}

/// Resolves a name and drops the addresses a tenant's endpoint must not
/// reach, so a name that passed every check when it was saved cannot point
/// at them by the time a request is sent.
pub(super) struct Resolver;

impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let resolved = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let kept = kept(resolved, &host)?;
            Ok(Box::new(kept.into_iter()) as Addrs)
        })
    }
}

/// Follows redirects as reqwest does, except to an address or host a
/// tenant's endpoint must not reach. A name is resolved by [`Resolver`]; a
/// literal address never is, so it is checked here.
pub(super) fn redirects() -> Policy {
    redirect::Policy::custom(|attempt: Attempt<'_>| {
        if attempt.previous().len() >= REDIRECTS {
            attempt.error("too many redirects")
        } else if is_url(attempt.url()) {
            attempt.error("redirected to a link-local or cloud metadata address")
        } else {
            attempt.follow()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_local_and_metadata_addresses_are_refused() {
        for refused in [
            "169.254.169.254",
            "169.254.0.1",
            "fe80::1",
            "febf::1",
            "fd00:ec2::254",
            "::ffff:169.254.169.254",
            "100.100.100.200",
        ] {
            let address: IpAddr = refused.parse().expect("an address");
            assert!(is_address(address), "{refused}");
        }
        for allowed in [
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.20",
            "172.17.0.1",
            "::1",
            "fd00::1",
            "fd00:ec2::253",
            "100.100.100.100",
            "8.8.8.8",
        ] {
            let address: IpAddr = allowed.parse().expect("an address");
            assert!(!is_address(address), "{allowed}");
        }
    }

    #[test]
    fn metadata_hosts_are_refused_however_they_are_spelled() {
        for refused in [
            "metadata.google.internal",
            "METADATA.google.internal.",
            "metadata.goog",
            "169.254.169.254",
            "[fe80::1]",
            "[fd00:ec2::254]",
            "[::ffff:169.254.169.254]",
        ] {
            assert!(is_host(refused), "{refused}");
        }
        for allowed in [
            "localhost",
            "litellm",
            "metadata",
            "ollama.lan",
            "gateway.internal",
            "api.openai.com",
            "[::1]",
            "192.168.1.20",
        ] {
            assert!(!is_host(allowed), "{allowed}");
        }
    }

    #[test]
    fn a_name_resolving_only_to_link_local_addresses_is_refused() {
        let resolved = [
            SocketAddr::from(([169, 254, 169, 254], 0)),
            "[fe80::1]:0".parse().expect("an address"),
        ];

        let refused = kept(resolved.into_iter(), "rebound.example");

        assert!(
            refused.is_err_and(|error| error.contains("rebound.example")),
            "a name answering only with link-local addresses must not be reached"
        );
    }

    #[test]
    fn a_link_local_address_beside_a_reachable_one_is_dropped() {
        let reachable = SocketAddr::from(([192, 168, 1, 20], 0));
        let resolved = [SocketAddr::from(([169, 254, 169, 254], 0)), reachable];

        assert_eq!(
            kept(resolved.into_iter(), "mixed.example"),
            Ok(vec![reachable])
        );
    }

    #[tokio::test]
    async fn the_resolver_keeps_loopback() {
        let name: Name = "localhost".parse().expect("a name");

        let resolved = Resolver.resolve(name).await;

        assert!(resolved.is_ok_and(|mut addresses| {
            addresses
                .next()
                .is_some_and(|address| address.ip().is_loopback())
        }));
    }
}
