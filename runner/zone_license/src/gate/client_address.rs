use std::net::{IpAddr, SocketAddr};

use axum::http::{HeaderMap, HeaderName};

use crate::gate::trusted_proxies::TrustedProxies;

pub const FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// Only the rightmost `X-Forwarded-For` entry is taken, because that is the
/// one the trusted proxy appended; everything left of it came from the client.
/// A trusted peer that forwards no usable entry is itself the client.
pub fn client_address(
    peer: Option<SocketAddr>,
    headers: &HeaderMap,
    proxies: &TrustedProxies,
) -> Option<IpAddr> {
    let peer = peer?.ip().to_canonical();
    if !proxies.trusts(peer) {
        return Some(peer);
    }
    Some(forwarded(headers).unwrap_or(peer))
}

fn forwarded(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get_all(FORWARDED_FOR).iter().next_back()?;
    let entry = value.to_str().ok()?.rsplit(',').next()?.trim();
    entry
        .parse::<IpAddr>()
        .or_else(|_| entry.parse::<SocketAddr>().map(|socket| socket.ip()))
        .ok()
        .map(|address| address.to_canonical())
}
