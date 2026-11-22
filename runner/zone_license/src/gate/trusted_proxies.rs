use std::fmt;
use std::net::IpAddr;

use ipnet::IpNet;

use crate::gate::unparsable::Unparsable;

pub const SEPARATOR: char = ',';
pub const NONE: &str = "none";

const EXPECTED: &str = "a CIDR range or an IP address, such as 172.18.0.0/16";

/// A bare address is trusted as a single host. Host bits in a range are
/// dropped, so `172.18.0.1/16` trusts all of `172.18.0.0/16`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<IpNet>);

impl TrustedProxies {
    pub fn parse(list: &str) -> Result<Self, Unparsable> {
        if list.trim().eq_ignore_ascii_case(NONE) {
            return Ok(Self::default());
        }
        list.split(SEPARATOR)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                entry
                    .parse::<IpNet>()
                    .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
                    .map(|range| range.trunc())
                    .map_err(|_| Unparsable {
                        entry: entry.to_owned(),
                        expected: EXPECTED,
                    })
            })
            .collect::<Result<Vec<IpNet>, Unparsable>>()
            .map(Self)
    }

    pub fn trusts(&self, peer: IpAddr) -> bool {
        let peer = peer.to_canonical();
        self.0.iter().any(|range| range.contains(&peer))
    }

    pub fn iter(&self) -> impl Iterator<Item = &IpNet> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for TrustedProxies {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return formatter.write_str(NONE);
        }
        for (index, range) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(",")?;
            }
            write!(formatter, "{range}")?;
        }
        Ok(())
    }
}
