mod client_address;
mod code;
mod credentials;
mod denial;
mod detail;
mod errors;
mod limited;
mod limits;
mod refusal;
mod revoked;
mod throttle;
mod throttle_key;
mod trusted_proxies;
mod unparsable;

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::{Feature, Keyring, License, Status};

use crate::variable::Variable;

pub use client_address::{FORWARDED_FOR, client_address};
pub use code::Code;
pub use credentials::{Credentials, SCHEME};
pub use denial::Denial;
pub use detail::Detail;
pub use errors::Errors;
pub use limited::Limited;
pub use limits::{DEFAULT_ADDRESS_LIMIT, DEFAULT_LICENSE_LIMIT, Limits, WINDOW};
pub use refusal::{
    ANONYMOUS, CHALLENGE, INELIGIBLE, LAPSED, MALFORMED, MISMATCHED, REVOKED, Refusal, UNVERIFIED,
};
pub use revoked::Revoked;
pub use throttle::Throttle;
pub use throttle_key::{IPV6_PREFIX, ThrottleKey};
pub use trusted_proxies::TrustedProxies;
pub use unparsable::Unparsable;

/// Admits only a verified, unrevoked key presented under its own license ID,
/// on a plan that includes distribution, while active or in grace. The plan is
/// checked before the status, so a lapsed Team key is refused as ineligible.
#[derive(Debug, Clone)]
pub struct Gate {
    keyring: Keyring,
    revoked: Revoked,
    proxies: TrustedProxies,
    clock: fn() -> DateTime<Utc>,
}

impl Gate {
    pub fn new(keyring: Keyring, revoked: Revoked, proxies: TrustedProxies) -> Self {
        Self {
            keyring,
            revoked,
            proxies,
            clock: Utc::now,
        }
    }

    pub fn with_clock(self, clock: fn() -> DateTime<Utc>) -> Self {
        Self { clock, ..self }
    }

    pub fn proxies(&self) -> &TrustedProxies {
        &self.proxies
    }

    pub fn client(&self, peer: Option<SocketAddr>, headers: &HeaderMap) -> Option<IpAddr> {
        client_address(peer, headers, &self.proxies)
    }

    pub fn admit(&self, headers: &HeaderMap) -> Result<License, Denial> {
        let credentials = Credentials::from_headers(headers).map_err(Denial::new)?;
        let license = self
            .keyring
            .verify(credentials.password().trim())
            .map_err(|error| Denial::unverified(error.code()))?;
        let now = (self.clock)();
        let refuse = |refusal: Refusal| Err(Denial::verified(refusal, &license, now));
        if self.revoked.contains(license.id()) {
            return refuse(Refusal::Revoked);
        }
        if Uuid::try_parse(credentials.user()).ok() != Some(license.id()) {
            return refuse(Refusal::Mismatched);
        }
        if !license.plan().includes(Feature::Distribution) {
            return refuse(Refusal::Ineligible);
        }
        if !matches!(license.status(now), Status::Active | Status::Grace) {
            return refuse(Refusal::Lapsed);
        }
        debug_assert!(Feature::Distribution.entitled(Some(&license), now));
        Ok(license)
    }
}

/// Call once at startup when a gated route is on. Without trusted proxies
/// every client behind Caddy shares Caddy's address, and so its limit.
pub fn announce(proxies: &TrustedProxies) {
    if proxies.is_empty() {
        tracing::warn!(
            variable = Variable::TrustedProxies.name(),
            "No trusted proxies are set, so every client behind a proxy shares one address limit"
        );
    } else {
        tracing::info!(
            proxies = %proxies,
            "Taking the client address from X-Forwarded-For when the peer is a trusted proxy"
        );
    }
}

#[cfg(test)]
mod tests;
