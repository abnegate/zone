use std::net::{IpAddr, Ipv6Addr};

pub const IPV6_PREFIX: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThrottleKey(IpAddr);

impl ThrottleKey {
    pub fn address(self) -> IpAddr {
        self.0
    }
}

impl From<IpAddr> for ThrottleKey {
    fn from(address: IpAddr) -> Self {
        match address.to_canonical() {
            IpAddr::V4(address) => Self(IpAddr::V4(address)),
            IpAddr::V6(address) => {
                let mask = u128::MAX << (u128::BITS - IPV6_PREFIX);
                Self(IpAddr::V6(Ipv6Addr::from_bits(address.to_bits() & mask)))
            }
        }
    }
}
